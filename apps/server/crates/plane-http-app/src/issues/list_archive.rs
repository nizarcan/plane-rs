// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/projects/{project_id}/archived-issues/` —
//! `IssueArchiveViewSet.list` (`plane/app/views/issue/archive.py:45-211`).
//!
//! The same paginated issue list as `IssueViewSet.list`, over a different
//! queryset:
//!
//! ```text
//! Issue.objects                                  # SoftDeletionManager: deleted_at IS NULL
//!   .filter(Q(type__isnull=True) | Q(type__is_epic=False))
//!   .filter(archived_at__isnull=False)
//!   .filter(project_id=…).filter(workspace__slug=…)
//!   [.filter(parent__isnull=True)]               # ?show_sub_issues != "true"
//!   .filter(<ComplexFilterBackend: ?filters=>)   .filter(**issue_filters(…))
//!   .annotate(cycle_id, link_count, attachment_count, sub_issues_count)
//!   order_issue_queryset(…) · issue_queryset_grouper(…) · paginate(…)
//! ```
//!
//! Three differences from `IssueViewSet.list` decide rows and are carried by
//! [`plane_query::ListOptions`]:
//!
//! * the manager predicate is replaced (archived rows only, epics excluded), so
//!   the triage/archived/draft exclusions do not apply;
//! * `get_queryset()` never calls `.distinct()`, so the counts are joined rows;
//! * `issue_group_values` is called **without** a queryset
//!   (`archive.py:158-166`), so grouping by `start_date`, `target_date` or
//!   `created_by` raises the Python's `AttributeError` (a 500 here).
//!
//! `apply_annotations` is `IssueViewSet`'s (`archive.py:56-86`), so the three
//! counts are `Subquery … GROUP BY` and render `null` for an issue with no
//! related row.

use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::model::Relation;
use plane_query::issue::query::{IssueScope, IssueScopeFilter};
use plane_query::issue::values::Projection;
use plane_query::issue::{FilterBackends, ListOptions, ListRequest};
use plane_query::sql::{Frag, JoinKind};
use uuid::Uuid;

use super::list_common::{
    client, kwarg_text, kwarg_uuid, query_error_response, rendered, user_id, user_timezone,
};

/// Register `IssueArchiveViewSet.list`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.archive.IssueArchiveViewSet")?
        .action("list", list)?;
    Ok(())
}

/// `IssueArchiveViewSet.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id: Uuid = kwarg_uuid(ctx, "project_id")?;
        let _user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);

        // `Issue.objects` is `SoftDeletionManager`: `deleted_at IS NULL`, and
        // nothing else (`plane/db/mixins.py`). The epic exclusion is a LEFT
        // OUTER join on `issue_types` (`archive.py:78-79`).
        let mut predicate = Frag::is_null("issues.deleted_at")
            .and(Frag::raw(
                "(issues.type_id IS NULL OR NOT issue_types.is_epic)",
            ))
            .and(Frag::is_not_null("issues.archived_at"));
        // `show_sub_issues = request.GET.get("show_sub_issues", "true")`, a
        // string comparison: any other value (including `True`) filters.
        if ctx.query().get("show_sub_issues").unwrap_or("true") != "true" {
            predicate = predicate.and(Frag::is_null("issues.parent_id"));
        }

        let options = ListOptions {
            // `filter_backends = (ComplexFilterBackend,)` (`archive.py:48`).
            filter_backends: FilterBackends::ComplexOnly,
            scope: Some(
                IssueScopeFilter::new(predicate).with_join(Relation::IssueTypes, JoinKind::Left),
            ),
            // `IssueViewSet.apply_annotations`' `Subquery … GROUP BY` counts.
            projection: Projection::Grouped,
            // `issue_group_values(field=…, slug=…, project_id=…, filters=…)`
            // with no queryset (`archive.py:158-166`).
            group_values_queryset: false,
            default_order_by: "-created_at",
            // Cycles/modules/archive filter first and paginate second, like
            // `IssueViewSet.list`.
            validate_page_first: false,
            grouping: true,
            scope_override: Some(IssueScope {
                workspace_slug: slug.to_owned(),
                project_id: Some(project_id),
                // `IssueArchiveViewSet.get_queryset` never calls `.distinct()`.
                distinct: false,
                created_by: None,
            }),
        };

        let request = ListRequest {
            query: ctx.query(),
            workspace_slug: slug,
            project_id,
            timezone,
            now: plane_query::global::now(),
            user_id: None,
            guest_view_all_features: true,
            is_guest: false,
        };

        match plane_query::issue::run_with(client, &request, &options).await {
            Ok(body) => Ok(rendered(200, &body)),
            Err(error) => match query_error_response(&error) {
                Some(response) => Ok(response),
                None => Err(ViewError::Unknown(error.message())),
            },
        }
    })
}
