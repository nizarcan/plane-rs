// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `CycleIssueViewSet.list` and `ModuleIssueViewSet.list`, the two issue lists
//! this port owns.
//!
//! Both are the `IssueViewSet.list` pipeline of `crates/plane-query` with a
//! different scope:
//!
//! ```text
//! Issue.issue_objects
//!   .filter(issue_cycle__cycle_id=…, issue_cycle__deleted_at__isnull=True)
//! (.filter(project_id=…).filter(workspace__slug=…) for the cycle list;
//!  ModuleIssueViewSet builds all three in get_queryset and adds .distinct())
//!   .filter(<ComplexFilterBackend: ?filters= only>)
//!   .filter(**issue_filters(…))
//!   .annotate(cycle_id, link_count, attachment_count, sub_issues_count)
//!   order_issue_queryset(…)
//!   issue_queryset_grouper(…)
//!   paginate(…)
//! ```
//!
//! Three facts about these views decide rows and are reproduced through
//! [`plane_query::ListOptions`]:
//!
//! * **`distinct`.** `CycleIssueViewSet.list` builds
//!   `Issue.issue_objects.filter(...)` directly, so its counts are joined rows;
//!   `ModuleIssueViewSet.get_queryset` ends in `.distinct()` and its counts
//!   collapse them (`plane/app/views/module/issue.py:91-100`).
//! * **The through-table scope is an inner join**, not an `EXISTS`: Django's
//!   `filter(issue_cycle__cycle_id=…)` joins the through table, so an issue
//!   with two matching rows would appear twice (`cycle/issue.py:107-112`).
//! * **Only `?filters=` filters.** `filter_backends` is `(ComplexFilterBackend,)`
//!   and `ComplexFilterBackend.filter_queryset` reads just the `filters` JSON
//!   tree (`plane/utils/filters/filter_backend.py:31-53`); the
//!   `IssueFilterSet`'s plain parameters (`assignee_id=…`, `state_id=…`) are
//!   ignored. Legacy `issue_filters` still reads `priority`, `state`, `labels`
//!   and friends, which is what makes those parameters work.
//!
//! The module list defaults `order_by` to `"created_at"` (`module/issue.py:113`)
//! while the cycle list defaults to `"-created_at"` (`cycle/issue.py:130`), and
//! only the module list passes a queryset to `issue_group_values`, so
//! `group_by=start_date|target_date|created_by` on a cycle list is an
//! `AttributeError` (500) there.

use chrono::Utc;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_query::error::QueryError;
use plane_query::issue::model::manager_predicate;
use plane_query::issue::query::{IssueScope, IssueScopeFilter};
use plane_query::issue::values::Projection;
use plane_query::issue::{FilterBackends, ListOptions, ListRequest};
use plane_query::sql::{Frag, JoinKind, Value};
use uuid::Uuid;

use crate::cycles::base::Params;

/// Which through table scopes the list.
#[derive(Clone, Copy)]
pub(crate) enum ScopeKind {
    /// `issue_cycle__cycle_id=…, issue_cycle__deleted_at IS NULL`.
    Cycle(Uuid),
    /// `issue_module__module_id=…, issue_module__deleted_at IS NULL`.
    Module(Uuid),
}

impl ScopeKind {
    fn table(self) -> (&'static str, &'static str) {
        match self {
            Self::Cycle(_) => ("cycle_issues", "cycle_id"),
            Self::Module(_) => ("module_issues", "module_id"),
        }
    }

    fn id(self) -> Uuid {
        match self {
            Self::Cycle(id) | Self::Module(id) => id,
        }
    }

    /// The join the scope predicate needs (`issue_cycle` / `issue_module`).
    fn relation(self) -> plane_query::issue::model::Relation {
        match self {
            Self::Cycle(_) => plane_query::issue::model::Relation::CycleIssues,
            Self::Module(_) => plane_query::issue::model::Relation::ModuleIssues,
        }
    }
}

/// The request rows a list needs from `ViewCtx`.
pub(crate) struct Request<'a> {
    pub(crate) params: Params,
    pub(crate) scope: ScopeKind,
    /// `request.GET.get("order_by", …)`: `-created_at` for cycles,
    /// `created_at` for modules.
    pub(crate) default_order: &'static str,
    /// Whether `issue_group_values` receives the filtered issue queryset. The
    /// module list does (`module/issue.py:151`), the cycle list does not
    /// (`cycle/issue.py:162`).
    pub(crate) group_values_issues: bool,
    /// `CycleIssueViewSet.list` never calls `.distinct()`;
    /// `ModuleIssueViewSet.get_queryset` does (`module/issue.py:100`).
    pub(crate) distinct: bool,
    pub(crate) timezone: chrono_tz::Tz,
    pub(crate) query: &'a plane_drf::QueryDict,
}

/// Render a `QueryError` the way the view's own exception path would.
///
/// * `Api` — a DRF `ParseError`/`ValidationError`: a plain message becomes
///   `{"detail": …}`, a coded filter error its `{"message","code"[,"errors"]}`
///   dict (`plane/utils/filters/filter_backend.py:155-180`).
/// * `BadRequest` — `Response({"error": …}, 400)` (`issue.py:147-150`).
/// * `Validation` — Django's `ValidationError` from a lookup value, rendered by
///   the family handler as `400 {"error": "Please provide valid detail"}`.
/// * everything else — an upstream 500.
pub(crate) fn query_error(error: QueryError) -> Result<ViewResponse, ViewError> {
    match error {
        QueryError::Api(api) => {
            let body = match api.detail() {
                plane_query::Json::Str(message) => {
                    plane_query::Json::object(vec![("detail", plane_query::Json::Str(message))])
                }
                other => other,
            };
            Ok(ViewResponse::raw(
                api.status,
                APPLICATION_JSON,
                body.to_drf(),
            ))
        }
        QueryError::BadRequest(message) => Ok(crate::cycles::base::error_body(400, &message)),
        QueryError::Validation => Err(ViewError::Validation),
        other => Err(ViewError::Unknown(other.to_string())),
    }
}

/// `Issue.issue_objects.filter(<through-table scope>)` as a scope filter.
///
/// The manager's own exclusions are ANDed in explicitly, because
/// [`IssueScopeFilter`] replaces the hardcoded manager predicate.
fn scoped_filter(scope: ScopeKind) -> IssueScopeFilter {
    let (table, column) = scope.table();
    let mut predicate = Frag::raw(format!("{table}.{column} = "));
    predicate.push(Frag::bind(Value::Uuid(scope.id())));
    predicate.push_raw(&format!(" AND {table}.deleted_at IS NULL"));
    IssueScopeFilter::new(manager_predicate().and(predicate))
        .with_join(scope.relation(), JoinKind::Inner)
}

/// `CycleIssueViewSet.list` / `ModuleIssueViewSet.list`.
pub(crate) async fn run(
    ctx: &ViewCtx<'_>,
    request: Request<'_>,
) -> Result<ViewResponse, ViewError> {
    let client = crate::common::client(ctx)?;
    let options = ListOptions {
        filter_backends: FilterBackends::ComplexOnly,
        scope: Some(scoped_filter(request.scope)),
        projection: Projection::Aggregate,
        group_values_queryset: request.group_values_issues,
        default_order_by: request.default_order,
        // Cycles/modules/archive filter first and paginate second, like
        // `IssueViewSet.list`.
        validate_page_first: false,
        grouping: true,
        scope_override: Some(IssueScope {
            workspace_slug: request.params.slug.clone(),
            project_id: Some(request.params.project_id),
            distinct: request.distinct,
            created_by: None,
        }),
    };
    let list_request = ListRequest {
        query: request.query,
        workspace_slug: &request.params.slug,
        project_id: request.params.project_id,
        timezone: request.timezone,
        now: Utc::now(),
        user_id: Some(request.params.user_id),
        // No guest restriction on these two views.
        guest_view_all_features: true,
        is_guest: false,
    };
    match plane_query::issue::run_with(client, &list_request, &options).await {
        Ok(body) => Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf())),
        Err(error) => query_error(error),
    }
}
