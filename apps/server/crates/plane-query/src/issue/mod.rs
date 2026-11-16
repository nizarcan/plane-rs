// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The issue read path: `plane/utils/issue_filters.py`, `filters/`,
//! `order_queryset.py`, `grouper.py` and `paginator.py`.
//!
//! The public entry point is [`run`]: it takes the same inputs a view has
//! (`workspace__slug`, `project_id`, the query parameters, the user's
//! timezone, the current instant) and returns the response body the Django
//! endpoint returns, byte for byte. [`run_with`] adds the switches a view
//! hardcodes: which filter backends run, whether the manager predicate is
//! replaced, the annotation shape and the default ordering. The pieces
//! underneath are public so a handler can compose a variant without the whole
//! list.

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
use crate::issue::query::{GroupPlan, IssueQuery, IssueScope, IssueScopeFilter};
use crate::issue::values::Projection;
use crate::json::Json;
use crate::paginator::{PageQuery, issue_list_page_with};

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

/// Which filter backends a view runs over the query parameters.
///
/// `IssueViewSet.list`, `IssueListEndpoint`, `IssuePaginatedViewSet`, the
/// archived list, the cycle and module issue lists and the workspace view list
/// all declare `filter_backends = (ComplexFilterBackend,)`
/// (`plane/app/views/issue/base.py:209,82`; `plane/utils/filters/filter_backend.py:31-53`):
/// only the `?filters=` JSON tree filters, and the `IssueFilterSet` merely
/// validates the tree's field names. Applying it to the plain query parameters
/// as well (the default, [`FilterBackends::LegacyAndComplex`]) turns
/// `priority=urgent,high` into a 400 that Django answers with the legacy
/// filter's rows, so a view must ask for what it declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterBackends {
    /// `IssueFilterSet` over the query parameters *and* `?filters=`, with the
    /// JSON tree winning when both are present. This is what `issue::run` has
    /// always done; kept for callers whose view declares
    /// `(DjangoFilterBackend, ComplexFilterBackend)` or nothing at all.
    #[default]
    LegacyAndComplex,
    /// `filter_backends = (ComplexFilterBackend,)`: only `?filters=`.
    ComplexOnly,
}

/// The view-level switches `IssueViewSet.list` hardcodes.
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub filter_backends: FilterBackends,
    /// A replacement for the `Issue.issue_objects` manager predicate, for the
    /// views whose queryset is not the manager's (archived issues, the cycle
    /// and module lists, the workspace-wide view list).
    pub scope: Option<IssueScopeFilter>,
    /// Which annotation shape the page projects.
    pub projection: Projection,
    /// Whether `issue_group_values` receives the filtered issue queryset. The
    /// module list and `IssueViewSet` pass it; the cycle and archived lists do
    /// not, so their `target_date`/`start_date`/`created_by` grouping is the
    /// Python's `AttributeError` (500).
    pub group_values_queryset: bool,
    /// `request.GET.get("order_by", …)`.
    pub default_order_by: &'static str,
    /// Whether the view groups at all. `IssueDetailEndpoint` and
    /// `WorkspaceViewIssuesViewSet` ignore `?group_by=`; parsing it anyway
    /// would add `issue_queryset_grouper`'s through-table join to a page that
    /// Django returns ungrouped (one row per assignee, label or module).
    pub grouping: bool,
    /// Whether the paginator's own validation runs before the filters'.
    ///
    /// `IssueViewSet.list` filters first and paginates second
    /// (`plane/app/views/issue/base.py:281-302`), which is the default; a view
    /// that calls `self.paginate` before `filter_queryset` (some list endpoints
    /// do) sees `per_page`/`cursor` errors first, and asks for that order here.
    pub validate_page_first: bool,
    /// The full scope, when it is not the one `ListRequest` describes
    /// (a workspace-wide list has no `project_id`, and a handler that already
    /// ran the guest check knows `created_by`). `None` builds
    /// `IssueScope { workspace_slug, project_id, distinct: true, created_by }`
    /// from `ListRequest`'s guest fields, which is `run`'s behaviour.
    pub scope_override: Option<IssueScope>,
}

impl Default for ListOptions {
    /// The behaviour `issue::run` has always had: both filter backends, the
    /// manager predicate, `issue_on_results`' annotations, a group-value
    /// queryset and `-created_at`.
    fn default() -> Self {
        Self {
            filter_backends: FilterBackends::LegacyAndComplex,
            scope: None,
            projection: Projection::Grouped,
            group_values_queryset: true,
            default_order_by: "-created_at",
            grouping: true,
            validate_page_first: false,
            scope_override: None,
        }
    }
}

impl ListOptions {
    /// What `IssueViewSet.list` does when nothing is overridden.
    pub fn issue_viewset() -> Self {
        Self {
            filter_backends: FilterBackends::ComplexOnly,
            scope: None,
            projection: Projection::Grouped,
            group_values_queryset: true,
            default_order_by: "-created_at",
            grouping: true,
            validate_page_first: false,
            scope_override: None,
        }
    }
}

/// `IssueViewSet.list`, end to end, with [`ListOptions::default`]'s
/// filter-backend selection (the behaviour this crate always had).
pub async fn run(client: &Client, request: &ListRequest<'_>) -> Result<Json, QueryError> {
    run_with(client, request, &ListOptions::default()).await
}

/// [`run`] with the view's own switches.
pub async fn run_with(
    client: &Client,
    request: &ListRequest<'_>,
    options: &ListOptions,
) -> Result<Json, QueryError> {
    let page_query = request.query;
    let group_by_param = page_query.get("group_by").filter(|value| !value.is_empty());
    let sub_group_by_param = page_query
        .get("sub_group_by")
        .filter(|value| !value.is_empty());
    let group_by = group_by_param.and_then(model::GroupField::parse);
    let sub_group_by = sub_group_by_param.and_then(model::GroupField::parse);

    // The Python's order, because it decides which error a request carrying
    // two mistakes sees:
    //
    //   `issue_filters`           builds a dict, cannot fail
    //   `filter_queryset`         the filterset/JSON tree: 400s here
    //   `.filter(**filters)`      a bad legacy date is `ValidationError` (400):
    //                             `DateField.get_prep_value` runs while the
    //                             filter is built, not when the SQL compiles
    //   group == sub group        `{"error": …}`, before `paginate`
    //   `paginate`                per_page, cursor, group_by allowlist: 400
    //
    // (`plane/app/views/issue/base.py:275-302,324-332`,
    // `django/db/models/sql/query.py:1261-1300`,
    // `plane/utils/paginator.py:681-687`.)
    //
    // A view that paginates before it filters asks for
    // `ListOptions::validate_page_first`, which moves the paginator's parameter
    // checks ahead of everything else.
    let early_page = if options.validate_page_first {
        Some(PageQuery::parse(
            page_query,
            group_by_param,
            sub_group_by_param,
        )?)
    } else {
        None
    };

    let rich = match options.filter_backends {
        FilterBackends::LegacyAndComplex => {
            let rich = rich::parse_filterset(page_query, request.timezone)?;
            match rich::parse_complex_query(page_query, request.timezone)? {
                Some(complex) => Some(complex),
                None => rich,
            }
        }
        FilterBackends::ComplexOnly => rich::parse_complex_query(page_query, request.timezone)?,
    };

    let legacy = LegacyFilters::parse(&FilterParams::Get(page_query), request.now.date_naive())?;

    if group_by.is_some() && group_by == sub_group_by {
        // `plane/app/views/issue/base.py:327`: a bare `{"error": …}` body, not
        // DRF's `{"detail": …}`.
        return Err(QueryError::BadRequest(
            "Group by and sub group by cannot have same parameters".to_owned(),
        ));
    }

    let page = match early_page {
        Some(page) => page,
        None => PageQuery::parse(page_query, group_by_param, sub_group_by_param)?,
    };
    let ordering = Ordering::resolve(page_query.get_or("order_by", options.default_order_by));

    // The guest restriction (`plane/app/views/issue/base.py:311-322`).
    let created_by = if request.is_guest && !request.guest_view_all_features {
        request.user_id
    } else {
        None
    };

    let query = IssueQuery {
        scope: options.scope_override.clone().unwrap_or(IssueScope {
            workspace_slug: request.workspace_slug.to_owned(),
            project_id: Some(request.project_id),
            distinct: true,
            created_by,
        }),
        filters: legacy,
        rich,
        ordering,
        group_by,
        sub_group_by,
        timezone: request.timezone,
    };

    let plan = match group_by {
        Some(field) => {
            let group_fields = fetch_group_values(client, &query, field, options).await?;
            let (sub_group_by, sub_group_fields) = match sub_group_by {
                Some(sub) => (
                    Some(sub),
                    fetch_group_values(client, &query, sub, options).await?,
                ),
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

    issue_list_page_with(client, &query, plan.as_ref(), &page, options).await
}

/// The group-key list for one field, with the options' queryset choice.
pub async fn fetch_group_values(
    client: &Client,
    query: &IssueQuery,
    field: model::GroupField,
    options: &ListOptions,
) -> Result<Vec<String>, QueryError> {
    let filtered = query.filtered_base_with(options.scope.as_ref());
    let request = group_values::GroupValuesRequest {
        field,
        workspace_slug: &query.scope.workspace_slug,
        project_id: query.scope.project_id,
        user: None,
        issues: if options.group_values_queryset {
            Some(&filtered)
        } else {
            None
        },
    };
    let plan = group_values::values(&request)?;
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

#[cfg(test)]
mod tests {
    use super::{FilterBackends, ListOptions};
    use crate::issue::values::Projection;

    #[test]
    fn filter_backends_are_explicit_per_view() {
        // `issue::run` keeps the behaviour it always had; every `/api/` issue
        // list declares `(ComplexFilterBackend,)`, so it asks for
        // `ComplexOnly` and the plain parameters stay with `issue_filters`
        // (`plane/app/views/issue/base.py:209,82`,
        // `plane/utils/filters/filter_backend.py:31-53`).
        assert_eq!(
            ListOptions::default().filter_backends,
            FilterBackends::LegacyAndComplex
        );
        assert_eq!(
            ListOptions::issue_viewset().filter_backends,
            FilterBackends::ComplexOnly
        );
        assert_eq!(ListOptions::issue_viewset().projection, Projection::Grouped);
        assert!(ListOptions::issue_viewset().grouping);
    }
}
