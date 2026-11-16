// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The issue read path: `plane/utils/issue_filters.py`, `filters/`,
//! `order_queryset.py`, `grouper.py` and `paginator.py`.
//!
//! The public entry point is [`run`]: it takes the same inputs a view has
//! (`workspace__slug`, `project_id`, the query parameters, the user's
//! timezone, the current instant) and returns the response body the Django
//! endpoint returns, byte for byte. The pieces underneath are public so a
//! handler can compose a variant (a cycle's issues, a module's) without the
//! whole list.

pub mod filter;
pub mod group_values;
pub mod model;
pub mod order;
pub mod query;
pub mod rich;
pub mod values;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_drf::QueryDict;
use uuid::Uuid;

use crate::error::QueryError;
use crate::issue::filter::{FilterParams, LegacyFilters};
use crate::issue::order::Ordering;
use crate::issue::query::{GroupPlan, IssueQuery, IssueScope};
use crate::json::Json;
use crate::paginator::{PageQuery, issue_list_page};

/// The parameters `IssueViewSet.list` reads.
#[derive(Debug, Clone)]
pub struct ListRequest<'a> {
    pub query: &'a QueryDict,
    pub workspace_slug: &'a str,
    pub project_id: Uuid,
    /// The user's profile timezone (`TimezoneMixin`).
    pub timezone: Tz,
    /// The instant relative date filters are measured from.
    pub now: DateTime<Utc>,
    /// `request.user.id`, for the guest restriction.
    pub user_id: Option<Uuid>,
    /// `project.guest_view_all_features`.
    pub guest_view_all_features: bool,
    /// Whether the caller is a guest (`ProjectMember.role == 5`).
    pub is_guest: bool,
}

/// `IssueViewSet.list`, end to end.
pub async fn run(client: &Client, request: &ListRequest<'_>) -> Result<Json, QueryError> {
    let page_query = request.query;
    let group_by_param = page_query.get("group_by").filter(|value| !value.is_empty());
    let sub_group_by_param = page_query
        .get("sub_group_by")
        .filter(|value| !value.is_empty());
    let page = PageQuery::parse(page_query, group_by_param, sub_group_by_param)?;

    let legacy = LegacyFilters::parse(&FilterParams::Get(page_query), request.now.date_naive())?;
    let rich = rich::parse_filterset(page_query, request.timezone)?;
    // `filter_queryset` runs the complex JSON filter too when the view is
    // configured with it; the issue list endpoint reads `?filters=` as well.
    let rich = match rich::parse_complex_query(page_query, request.timezone)? {
        Some(complex) => Some(complex),
        None => rich,
    };
    let ordering = Ordering::resolve(page_query.get_or("order_by", "-created_at"));
    let group_by = group_by_param.and_then(model::GroupField::parse);
    let sub_group_by = sub_group_by_param.and_then(model::GroupField::parse);

    if group_by.is_some() && group_by == sub_group_by {
        // `plane/app/views/issue/base.py:327`: a bare `{"error": …}` body, not
        // DRF's `{"detail": …}`.
        return Err(QueryError::BadRequest(
            "Group by and sub group by cannot have same parameters".to_owned(),
        ));
    }

    // The guest restriction (`plane/app/views/issue/base.py:311-322`).
    let created_by = if request.is_guest && !request.guest_view_all_features {
        request.user_id
    } else {
        None
    };

    let query = IssueQuery {
        scope: IssueScope {
            workspace_slug: request.workspace_slug.to_owned(),
            project_id: Some(request.project_id),
            distinct: true,
            created_by,
        },
        filters: legacy,
        rich,
        ordering,
        group_by,
        sub_group_by,
        timezone: request.timezone,
    };

    let plan = match group_by {
        Some(field) => {
            let group_fields = fetch_group_values(client, &query, field).await?;
            let (sub_group_by, sub_group_fields) = match sub_group_by {
                Some(sub) => (Some(sub), fetch_group_values(client, &query, sub).await?),
                None => (None, Vec::new()),
            };
            Some(GroupPlan {
                group_by: field,
                group_fields,
                sub_group_by,
                sub_group_fields,
            })
        }
        None => None,
    };

    issue_list_page(client, &query, plan.as_ref(), &page).await
}

async fn fetch_group_values(
    client: &Client,
    query: &IssueQuery,
    field: model::GroupField,
) -> Result<Vec<String>, QueryError> {
    let filtered = query.filtered_base();
    let plan = group_values::values(&query.group_value_request(field, &filtered))?;
    match plan {
        group_values::GroupValues::Static(values) => Ok(values),
        group_values::GroupValues::Query { sql, append_none } => {
            crate::stats::record();
            let statement = client.prepare(&sql.text).await?;
            let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
                .params
                .iter()
                .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
                .collect();
            let rows = client.query(&statement, &params).await?;
            let mut values = Vec::with_capacity(rows.len() + 1);
            for row in &rows {
                values.push(query::render_group_value(row, field)?);
            }
            if append_none {
                values.push("None".to_owned());
            }
            // `__get_field_dict` builds a dict from this list, so duplicate
            // keys collapse; keep the first-appearance order (the issue-derived
            // fields return one entry per issue).
            let mut seen = std::collections::HashSet::new();
            values.retain(|value| seen.insert(value.clone()));
            Ok(values)
        }
    }
}
