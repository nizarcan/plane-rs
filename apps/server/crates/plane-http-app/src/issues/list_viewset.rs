// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/projects/{project_id}/issues/` —
//! `IssueViewSet.list` (`plane/app/views/issue/base.py:280-391`).
//!
//! The statement assembly, filters, ordering, grouping and pagination are
//! `plane_query`'s; this module is the view around it:
//!
//! * `Project.objects.get(pk=project_id, workspace__slug=slug)` for
//!   `guest_view_all_features` and the `ProjectMember` guest check — one
//!   statement instead of two (`list_common::project_with_membership`);
//! * the one extra filter the ORM applies outside `issue_filters`
//!   (`updated_at__gt`, `:284-286`), passed as `ListRequest::extra`;
//! * `recent_visited_task.delay(...)` in the `ProjectKeyed` keyword order and
//!   with the argument types `TASKS.md` §8.2 records for this call site;
//! * the two 400 bodies: `{"error": "Group by and sub group by cannot have same
//!   parameters"}` from the view and DRF's `{"detail": …}` from the paginator.
//!
//! The `@method_decorator(gzip_page)` on the method is Django's global
//! `GZipMiddleware` applied twice; the pipeline already compresses, as it does
//! for Django, so there is nothing to do here.

use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::error::QueryError;
use plane_query::issue::query::IssueScope;
use plane_query::sql::{Frag, Value};

use super::list_common::{
    ListRequest, client, error_body, kwarg_text, kwarg_uuid, parse_django_datetime,
    prevalidate_list_filters, project_with_membership, query_error, query_error_response, rendered,
    run_issue_list, user_id, user_timezone,
};
use crate::common::jobs;

/// Register `IssueViewSet.list`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.base.IssueViewSet")?
        .action("list", list)?;
    Ok(())
}

/// `IssueViewSet.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `project = Project.objects.get(pk=project_id, workspace__slug=slug)`
        // (`base.py:296`), then the membership row the guest check reads.
        let project = project_with_membership(client, slug, project_id, user).await?;

        let timezone = user_timezone(ctx);
        let now = plane_query::global::now();

        // A date lookup Django rejects at compile time (a `ValidationError`
        // the family handler turns into its 400) is detected before anything
        // else; see `legacy_date_error`.
        if let Some(response) = super::list_common::legacy_date_error(ctx.query()) {
            return Ok(response);
        }

        // `issue_filters` and `filter_queryset` run before the task; the
        // paginator runs after it (`:292-322`).
        if let Err(error) = prevalidate_list_filters(ctx.query(), timezone, now) {
            return match query_error_response(&error) {
                Some(response) => Ok(response),
                None => Err(query_error(error)),
            };
        }

        // `updated_at__gt` is not a declared `IssueFilterSet` field: the view
        // adds it as a plain ORM lookup on top of the filterset (`:284-286`),
        // after the filterset ran and before the task is enqueued.
        let extra = match ctx.query().get("updated_at__gt") {
            Some(value) => Frag::compare(
                "issues.updated_at",
                ">",
                Value::Timestamp(parse_django_datetime(value, timezone)?),
            ),
            None => Frag::empty(),
        };

        // `recent_visited_task.delay(slug, project_id, entity_name="project",
        // entity_identifier=project_id, user_id=request.user.id)` (`:304`).
        jobs::recent_visited_project(
            ctx,
            "project",
            TaskArg::uuid(project_id),
            Some(TaskArg::uuid(project_id)),
            TaskArg::uuid(user),
        )
        .await?;

        let request = ListRequest {
            query: ctx.query(),
            scope: IssueScope {
                workspace_slug: slug.to_owned(),
                project_id: Some(project_id),
                distinct: true,
                created_by: if project.is_guest() && !project.guest_view_all_features {
                    Some(user)
                } else {
                    None
                },
            },
            timezone,
            now,
            extra,
        };

        match run_issue_list(client, &request).await {
            Ok(body) => Ok(rendered(200, &body)),
            Err(QueryError::BadRequest(message)) => Ok(error_body(400, &message)),
            Err(error) => match query_error_response(&error) {
                Some(response) => Ok(response),
                None => Err(query_error(error)),
            },
        }
    })
}
