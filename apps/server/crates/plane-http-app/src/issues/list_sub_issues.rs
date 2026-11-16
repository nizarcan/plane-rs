// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/`
//! — `SubIssuesEndpoint.get` (`plane/app/views/issue/sub_issue.py:36-205`).
//!
//! The one issue list that is not paginated and not filtered: it returns every
//! live sub-issue of the URL parent (scoped to the URL workspace and project)
//! plus a `state_distribution` map, and it shapes the body around a
//! `?group_by=` parameter with its own rules:
//!
//! * `group_by=assignees__ids` (the Python's spelling) buckets each issue under
//!   **every** assignee id, or under `"None"` when the array is empty;
//! * any other `group_by` is a plain dict lookup — `issue[group_by]` — so a name
//!   that the projection does not carry is a `KeyError` (500), and a name it
//!   does carry (`state_id`, `priority`, `state_group`, …) bucketed by Python's
//!   `str()`.
//!
//! Three details the annotations decide and this module reproduces:
//!
//! * every `COUNT` subquery is wrapped in `Coalesce(…, 0)` and the id arrays in
//!   `Coalesce(…, [])`, so an issue with nothing gets `0`/`[]` (the paginated
//!   lists render `null` for the counts);
//! * `assignee_ids` keeps only assignees with an active `project_members` row —
//!   no `deleted_at` condition, unlike `IssueListEndpoint`'s annotation;
//! * `module_ids` drops modules with `archived_at` set.
//!
//! `state_distribution` is a `defaultdict(list)` keyed by `state_group`, which
//! is `None` for a stateless issue and therefore renders as the key `"null"`
//! (`json.dumps({None: []})`).

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::model::ValueField;
use plane_query::issue::order::Ordering;
use plane_query::issue::query::IssueScope;
use plane_query::issue::values::Projection;
use plane_query::json::Json;
use plane_query::sql::{Frag, Select, Value};
use uuid::Uuid;

use super::list_common::{client, kwarg_text, kwarg_uuid, rendered, user_id, user_timezone};

/// Register `SubIssuesEndpoint.get`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.issue.sub_issue.SubIssuesEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// The `.values()` order (`plane/app/views/issue/sub_issue.py:146-173`).
///
/// `state_group` is last, and the id arrays come *before* `sub_issues_count` —
/// unlike `issue_on_results`, whose keys the paginated lists render.
const PROJECTION: [Column; 26] = [
    Column::Field(ValueField::Id),
    Column::Field(ValueField::Name),
    Column::Field(ValueField::StateId),
    Column::Field(ValueField::SortOrder),
    Column::Field(ValueField::CompletedAt),
    Column::Field(ValueField::EstimatePoint),
    Column::Field(ValueField::Priority),
    Column::Field(ValueField::StartDate),
    Column::Field(ValueField::TargetDate),
    Column::Field(ValueField::SequenceId),
    Column::Field(ValueField::ProjectId),
    Column::Field(ValueField::ParentId),
    Column::Field(ValueField::CycleId),
    Column::ModuleIds,
    Column::LabelIds,
    Column::AssigneeIds,
    Column::SubIssuesCount,
    Column::Field(ValueField::CreatedAt),
    Column::Field(ValueField::UpdatedAt),
    Column::Field(ValueField::CreatedBy),
    Column::Field(ValueField::UpdatedBy),
    Column::AttachmentCount,
    Column::LinkCount,
    Column::Field(ValueField::IsDraft),
    Column::Field(ValueField::ArchivedAt),
    Column::StateGroup,
];

#[derive(Clone, Copy)]
enum Column {
    /// A column that is the same expression in every projection.
    Field(ValueField),
    /// `Coalesce(Subquery(ModuleIssue … archived_at IS NULL), [])`.
    ModuleIds,
    /// `Coalesce(Subquery(IssueLabel …), [])`.
    LabelIds,
    /// `Coalesce(Subquery(IssueAssignee … member_project__is_active …), [])`.
    AssigneeIds,
    /// `Coalesce(Subquery(Issue.issue_objects.filter(parent=…)), 0)`.
    SubIssuesCount,
    /// `Coalesce(Subquery(IssueLink …), 0)`.
    LinkCount,
    /// `Coalesce(Subquery(FileAsset …), 0)`.
    AttachmentCount,
    /// `states.group` (key `state_group`).
    StateGroup,
}

impl Column {
    fn key(self) -> &'static str {
        match self {
            Self::Field(field) => field.key(),
            Self::ModuleIds => "module_ids",
            Self::LabelIds => "label_ids",
            Self::AssigneeIds => "assignee_ids",
            Self::SubIssuesCount => "sub_issues_count",
            Self::LinkCount => "link_count",
            Self::AttachmentCount => "attachment_count",
            Self::StateGroup => "state_group",
        }
    }

    fn expression(self) -> Frag {
        match self {
            Self::Field(field) => {
                // `state__group` is not projected by this endpoint; the
                // `state_group` alias below carries it instead. Every other
                // field uses the shared `issue_on_results` expression (the
                // three counts are their own `Column`s, so their `GROUP BY`
                // difference does not matter here).
                match field {
                    ValueField::StateGroup => Frag::raw("states.group"),
                    _ => plane_query::issue::values::field_expression(Projection::Grouped, field),
                }
            }
            Self::ModuleIds => Frag::raw(
                "COALESCE((SELECT ARRAY_AGG(DISTINCT module_issues.module_id) \
                 FROM module_issues INNER JOIN modules ON module_issues.module_id = modules.id \
                 WHERE module_issues.deleted_at IS NULL AND module_issues.issue_id = issues.id \
                 AND modules.archived_at IS NULL GROUP BY module_issues.issue_id), ARRAY[]::uuid[])",
            ),
            Self::LabelIds => Frag::raw(
                "COALESCE((SELECT ARRAY_AGG(DISTINCT issue_labels.label_id) \
                 FROM issue_labels WHERE issue_labels.deleted_at IS NULL \
                 AND issue_labels.issue_id = issues.id GROUP BY issue_labels.issue_id), ARRAY[]::uuid[])",
            ),
            Self::AssigneeIds => Frag::raw(
                "COALESCE((SELECT ARRAY_AGG(DISTINCT issue_assignees.assignee_id) \
                 FROM issue_assignees INNER JOIN users ON issue_assignees.assignee_id = users.id \
                 INNER JOIN project_members ON users.id = project_members.member_id \
                 WHERE issue_assignees.deleted_at IS NULL AND project_members.is_active \
                 AND issue_assignees.issue_id = issues.id \
                 GROUP BY issue_assignees.issue_id), ARRAY[]::uuid[])",
            ),
            Self::SubIssuesCount => Frag::raw(
                "COALESCE((SELECT COUNT(sub_issues.id) FROM issues sub_issues \
                 LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id \
                 INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id \
                 WHERE sub_issues.deleted_at IS NULL \
                 AND (sub_states.group IS NULL OR sub_states.group <> 'triage') \
                 AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL \
                 AND NOT sub_issues.is_draft AND sub_issues.parent_id = issues.id \
                 GROUP BY sub_issues.parent_id), 0)",
            ),
            Self::LinkCount => Frag::raw(
                "COALESCE((SELECT COUNT(issue_links.id) FROM issue_links \
                 WHERE issue_links.deleted_at IS NULL AND issue_links.issue_id = issues.id \
                 GROUP BY issue_links.issue_id), 0)",
            ),
            Self::AttachmentCount => Frag::raw(
                "COALESCE((SELECT COUNT(file_assets.id) FROM file_assets \
                 WHERE file_assets.deleted_at IS NULL \
                 AND file_assets.entity_type = 'ISSUE_ATTACHMENT' \
                 AND file_assets.issue_id = issues.id GROUP BY file_assets.issue_id), 0)",
            ),
            Self::StateGroup => Frag::raw("states.group"),
        }
    }
}

/// `SubIssuesEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id: Uuid = kwarg_uuid(ctx, "project_id")?;
        let issue_id: Uuid = kwarg_uuid(ctx, "issue_id")?;
        let _user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);

        // `Issue.issue_objects.filter(parent_id=…, workspace__slug=…, project_id=…)`
        // (`sub_issue.py:40-44`), with the URL parent bound so a parent from
        // another project cannot leak sub-issue metadata across tenants.
        let scope = IssueScope {
            workspace_slug: slug.to_owned(),
            project_id: Some(project_id),
            // No `.distinct()` and no grouper predicate.
            distinct: false,
            created_by: None,
        };
        let query = plane_query::issue::query::IssueQuery {
            scope,
            filters: Default::default(),
            rich: None,
            ordering: Ordering::resolve(ctx.query().get_or("order_by", "-created_at")),
            group_by: None,
            sub_group_by: None,
            timezone,
        };
        let mut select = query.filtered_select();
        select.predicate = select.predicate.take().map(|predicate| {
            predicate.and(Frag::compare(
                "issues.parent_id",
                "=",
                Value::Uuid(issue_id),
            ))
        });
        select.projection = PROJECTION
            .iter()
            .map(|column| {
                let mut frag = column.expression();
                frag.push_raw(" AS \"");
                frag.push_raw(column.key());
                frag.push_raw("\"");
                frag
            })
            .collect();
        // The ordinal → expression map `Ordering::apply_own_order` uses: each
        // entry names the `ValueField` whose ordering expression this column
        // projects, so `state__group` lands on position 26, not 23.
        let fields: Vec<ValueField> = PROJECTION
            .iter()
            .map(|column| match column {
                Column::Field(field) => *field,
                Column::ModuleIds => ValueField::ModuleIds,
                Column::LabelIds => ValueField::LabelIds,
                Column::AssigneeIds => ValueField::AssigneeIds,
                Column::SubIssuesCount => ValueField::SubIssuesCount,
                Column::LinkCount => ValueField::LinkCount,
                Column::AttachmentCount => ValueField::AttachmentCount,
                Column::StateGroup => ValueField::StateGroup,
            })
            .collect();
        // `order_issue_queryset` applies its own ordering (no paginator's
        // `NULLS LAST` rule), by ordinal so a parameterised CASE expression
        // keeps working under the projection.
        query.ordering.apply_own_order(&mut select, &fields);
        if query.ordering.group_by_issue {
            select.group_by = vec!["issues.id".to_owned()];
        }

        let rows = fetch(client, &select).await?;
        let issues: Vec<Json> = rows
            .iter()
            .map(|row| decode_row(row, timezone))
            .collect::<Result<_, _>>()?;

        // `result = defaultdict(list)` over `state_group`, in row order.
        let mut state_distribution: Vec<(String, Vec<Json>)> = Vec::new();
        for issue in &issues {
            let key = json_key(&object_key(issue, "state_group"));
            push_bucket(
                &mut state_distribution,
                &key,
                Json::string(str_of(object_id(issue))),
            );
        }

        let group_by = ctx.query().get("group_by").unwrap_or_default();
        let body = if group_by.is_empty() {
            Json::object(vec![
                ("sub_issues", Json::Array(issues)),
                ("state_distribution", buckets(state_distribution)),
            ])
        } else {
            let mut grouped: Vec<(String, Vec<Json>)> = Vec::new();
            for issue in issues {
                if group_by == "assignees__ids" {
                    let assignees = match object_key(&issue, "assignee_ids") {
                        Json::Array(items) if !items.is_empty() => items,
                        _ => vec![Json::string("None")],
                    };
                    for assignee in assignees {
                        push_bucket(&mut grouped, &python_str(&assignee), issue.clone());
                    }
                } else {
                    // `issue[group_by]`: a name the projection does not carry is
                    // a `KeyError`, and the view family's `handle_exception`
                    // renders every `KeyError` as
                    // `400 {"error": "The required key does not exist."}`
                    // (`plane/app/views/base.py`'s exception table).
                    let Some(value) = object_get(&issue, group_by) else {
                        return Err(ViewError::Key);
                    };
                    push_bucket(&mut grouped, &python_str(value), issue.clone());
                }
            }
            Json::object(vec![
                ("sub_issues", buckets(grouped)),
                ("state_distribution", buckets(state_distribution)),
            ])
        };
        Ok(rendered(200, &body))
    })
}

/// `defaultdict(list)` insertion: first appearance of the key fixes its place.
fn push_bucket(buckets: &mut Vec<(String, Vec<Json>)>, key: &str, issue: Json) {
    match buckets.iter_mut().find(|(existing, _)| existing == key) {
        Some((_, issues)) => issues.push(issue),
        None => buckets.push((key.to_owned(), vec![issue])),
    }
}

fn buckets(buckets: Vec<(String, Vec<Json>)>) -> Json {
    Json::Object(
        buckets
            .into_iter()
            .map(|(key, ids)| (key, Json::Array(ids)))
            .collect(),
    )
}

fn object_id(value: &Json) -> Option<&Json> {
    object_get(value, "id")
}

fn object_get<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn object_key(value: &Json, key: &str) -> Json {
    object_get(value, key).cloned().unwrap_or(Json::Null)
}

fn str_of(value: Option<&Json>) -> String {
    value.map(python_str).unwrap_or_else(|| "None".to_owned())
}

/// A JSON object key for a value that Python used as a `dict` key directly:
/// `None` becomes `"null"`, a bool `"true"`/`"false"`, a number its `str()`.
fn json_key(value: &Json) -> String {
    match value {
        Json::Null => "null".to_owned(),
        Json::Bool(true) => "true".to_owned(),
        Json::Bool(false) => "false".to_owned(),
        other => python_str(other),
    }
}

/// Python's `str()` for the value types the projection carries.
fn python_str(value: &Json) -> String {
    match value {
        Json::Str(text) => text.clone(),
        Json::Null => "None".to_owned(),
        Json::Int(number) => number.to_string(),
        Json::Bool(flag) => {
            if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Json::Float(number) => plane_query::pyfloat::repr(*number),
        Json::Date(date) => date.to_string(),
        Json::DateTime(value) => value.with_timezone(&Utc).to_rfc3339(),
        Json::DateTimeZoned(value) => value.to_rfc3339(),
        // `group_by=module_ids` stringifies the `values()` list, and Django's
        // `ArrayField(UUIDField)` returns `uuid.UUID` objects, so the Python
        // repr is `[UUID('…')]`, not JSON.
        Json::Array(items) => {
            let parts: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Json::Object(_) => value.to_serde().to_string(),
    }
}

/// Python's `repr()` for a list element.
fn python_repr(value: &Json) -> String {
    match value {
        Json::Str(text) => match Uuid::parse_str(text) {
            Ok(uuid) => format!("UUID('{uuid}')"),
            Err(_) => {
                let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
                format!("'{escaped}'")
            }
        },
        Json::Null => "None".to_owned(),
        Json::Bool(true) => "True".to_owned(),
        Json::Bool(false) => "False".to_owned(),
        Json::Int(number) => number.to_string(),
        Json::Float(number) => plane_query::pyfloat::repr(*number),
        other => python_str(other),
    }
}

/// One row of the projection, in `PROJECTION` order.
fn decode_row(row: &tokio_postgres::Row, timezone: Tz) -> Result<Json, ViewError> {
    let mut entries: Vec<(&'static str, Json)> = Vec::with_capacity(PROJECTION.len());
    for (index, column) in PROJECTION.iter().enumerate() {
        let value = match column {
            Column::Field(ValueField::Id) | Column::Field(ValueField::ProjectId) => {
                Json::uuid(row.get(index))
            }
            Column::Field(ValueField::Name) | Column::Field(ValueField::Priority) => {
                Json::string(row.get::<_, String>(index))
            }
            Column::Field(ValueField::StateId)
            | Column::Field(ValueField::ParentId)
            | Column::Field(ValueField::CycleId)
            | Column::Field(ValueField::EstimatePoint)
            | Column::Field(ValueField::CreatedBy)
            | Column::Field(ValueField::UpdatedBy) => {
                Json::opt_uuid(row.get::<_, Option<Uuid>>(index))
            }
            Column::Field(ValueField::SortOrder) => Json::Float(row.get(index)),
            Column::Field(ValueField::CompletedAt) => shift(
                Json::opt_datetime(row.get::<_, Option<DateTime<Utc>>>(index)),
                timezone,
            ),
            Column::Field(ValueField::CreatedAt) | Column::Field(ValueField::UpdatedAt) => {
                shift(Json::DateTime(row.get::<_, DateTime<Utc>>(index)), timezone)
            }
            Column::Field(ValueField::StartDate)
            | Column::Field(ValueField::TargetDate)
            | Column::Field(ValueField::ArchivedAt) => {
                Json::opt_date(row.get::<_, Option<NaiveDate>>(index))
            }
            Column::Field(ValueField::SequenceId) => Json::Int(i64::from(row.get::<_, i32>(index))),
            Column::Field(ValueField::IsDraft) => Json::Bool(row.get(index)),
            Column::Field(ValueField::StateGroup) | Column::StateGroup => {
                Json::opt_string(row.get::<_, Option<String>>(index))
            }
            Column::ModuleIds | Column::LabelIds | Column::AssigneeIds => {
                let mut ids: Vec<Uuid> = row.get::<_, Option<Vec<Uuid>>>(index).unwrap_or_default();
                ids.sort();
                Json::Array(ids.into_iter().map(Json::uuid).collect())
            }
            Column::SubIssuesCount | Column::LinkCount | Column::AttachmentCount => {
                Json::Int(row.get::<_, i64>(index))
            }
            Column::Field(_) => {
                // Every projected field is matched above; a new `ValueField`
                // must be given a decoder before it can reach this endpoint.
                return Err(ViewError::Unknown(format!(
                    "unexpected projection field {:?}",
                    column.key()
                )));
            }
        };
        entries.push((column.key(), value));
    }
    Ok(Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    ))
}

/// `user_timezone_converter`'s `astimezone()` for the two datetime fields.
fn shift(value: Json, timezone: Tz) -> Json {
    match value {
        Json::DateTime(value) => Json::DateTimeZoned(value.with_timezone(&timezone).fixed_offset()),
        other => other,
    }
}

async fn fetch(client: &Client, select: &Select) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.clone().into_sql();
    let statement = client.prepare(&sql.text).await.map_err(ViewError::from)?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    client
        .query(&statement, &params)
        .await
        .map_err(ViewError::from)
}

#[cfg(test)]
mod tests {
    use super::{json_key, python_str};
    use plane_query::json::Json;

    #[test]
    fn a_list_group_value_uses_python_repr() {
        // `group_by=module_ids`: `str([UUID('…'), …])`.
        let value = Json::Array(vec![
            Json::string("6a68d88a-c0eb-4f42-9cab-84c915488824"),
            Json::string("3e6f9c26-e729-4009-ab69-974b067075aa"),
        ]);
        assert_eq!(
            python_str(&value),
            "[UUID('6a68d88a-c0eb-4f42-9cab-84c915488824'), \
             UUID('3e6f9c26-e729-4009-ab69-974b067075aa')]"
        );
        assert_eq!(python_str(&Json::Array(vec![Json::Null])), "[None]");
        assert_eq!(python_str(&Json::Array(vec![Json::Int(2)])), "[2]");
    }

    #[test]
    fn a_none_state_group_is_the_json_key_null() {
        // `result[sub_issue["state_group"]]` keys a Python dict by `None`, and
        // `JSONRenderer` encodes that key as `"null"` (`json.dumps`).
        assert_eq!(json_key(&Json::Null), "null");
        // The `group_by` branch stringifies with `str()`, so the same value is
        // the string `"None"` there.
        assert_eq!(python_str(&Json::Null), "None");
        assert_eq!(json_key(&Json::string("started")), "started");
        assert_eq!(json_key(&Json::Int(3)), "3");
    }
}
