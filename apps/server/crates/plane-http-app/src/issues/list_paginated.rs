// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/projects/{project_id}/v2/issues/` —
//! `IssuePaginatedViewSet.list` (`plane/app/views/issue/base.py:772-967`).
//!
//! The global paginator (`plane/utils/global_paginator.py`) with the user's
//! timezone applied to `created_at`/`updated_at`; `plane_query::global::page`
//! implements it, and `page_with` takes the projection in this view's own
//! `required_fields` order plus the optional `description_html` column
//! (`:885-902`) — `on_result` is `results.values(*required_fields)`, so the
//! column list, its order and the appended column are all the caller's.
//!
//! The guest restriction and `updated_at__gt` are plain ORM filters on both
//! querysets (`:965-967`), passed as extra predicates.
//!
//! Everything else — the cursor format, the clamping of `page_size`, the
//! `next_cursor: null` on the last page, `page_count`, `total_pages`, the
//! envelope key order and the local-time rendering — is
//! `plane_query::global`'s.

use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::global::ExtraColumn;
use plane_query::issue::model::ValueField;
use plane_query::issue::query::{IssueQuery, IssueScope};
use plane_query::json::Json;
use plane_query::sql::{Frag, Value};

use crate::issues::lists::list_common::{
    ListRequest, client, kwarg_text, kwarg_uuid, parse_django_datetime, project_with_membership,
    rendered, user_id, user_timezone,
};

/// Register `IssuePaginatedViewSet.list`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.base.IssuePaginatedViewSet")?
        .action("list", list)?;
    Ok(())
}

/// `IssuePaginatedViewSet.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = user_timezone(ctx);

        // `Project.objects.get(pk=project_id, workspace__slug=slug)`, then the
        // `ProjectMember` guest check (`:914-925`).
        let project = project_with_membership(client, slug, project_id, user).await?;
        let created_by = if project.is_guest() && !project.guest_view_all_features {
            Some(user)
        } else {
            None
        };

        let mut extra = Frag::empty();
        if let Some(value) = ctx.query().get("updated_at__gt") {
            extra = Frag::and(
                extra,
                Frag::compare(
                    "issues.updated_at",
                    ">",
                    Value::Timestamp(parse_django_datetime(value, timezone)?),
                ),
            );
        }

        let request = ListRequest {
            query: ctx.query(),
            scope: IssueScope {
                workspace_slug: slug.to_owned(),
                project_id: Some(project_id),
                // `get_queryset()` has no `.distinct()` here.
                distinct: false,
                created_by,
            },
            timezone,
            now: plane_query::global::now(),
            extra,
            // `IssuePaginatedViewSet` declares no filter backends at all, but
            // its `get_queryset` is `Issue.issue_objects.filter(...)`: the
            // `filterset_class` is only used by `IssueFilterSet`, which this
            // view never runs. Only `?filters=` filters (`base.py:819`).
            options: plane_query::ListOptions {
                filter_backends: plane_query::FilterBackends::ComplexOnly,
                ..plane_query::ListOptions::default()
            },
        };
        let query = super::list_common::build_issue_query(&request)
            .map_err(|error| ViewError::Unknown(error.message()))?;

        let description = ctx.query().get_or("description", "false").to_lowercase() == "true";
        let cursor = ctx.query().get("cursor");

        let body = page(client, &query, description, cursor, timezone).await?;
        Ok(rendered(200, &body))
    })
}

/// `IssuePaginatedViewSet.list`'s `required_fields`, in the Python's order
/// (`plane/app/views/issue/base.py:885-908`) — `state__group` is fourth and the
/// id arrays and counts close the object, unlike `issue_on_results`.
fn v2_fields() -> Vec<ValueField> {
    vec![
        ValueField::Id,
        ValueField::Name,
        ValueField::StateId,
        ValueField::StateGroup,
        ValueField::SortOrder,
        ValueField::CompletedAt,
        ValueField::EstimatePoint,
        ValueField::Priority,
        ValueField::StartDate,
        ValueField::TargetDate,
        ValueField::SequenceId,
        ValueField::ProjectId,
        ValueField::ParentId,
        ValueField::CycleId,
        ValueField::CreatedAt,
        ValueField::UpdatedAt,
        ValueField::CreatedBy,
        ValueField::UpdatedBy,
        ValueField::IsDraft,
        ValueField::ArchivedAt,
        ValueField::ModuleIds,
        ValueField::LabelIds,
        ValueField::AssigneeIds,
        ValueField::LinkCount,
        ValueField::AttachmentCount,
        ValueField::SubIssuesCount,
    ]
}
/// `paginate(base_queryset, queryset, cursor, on_result)`.
///
/// The projection is the caller's `required_fields`, in order, plus
/// `description_html` appended after `sub_issues_count` when
/// `?description=true` (`plane/app/views/issue/base.py:900-902`) — that is what
/// `on_result`'s `results.values(*required_fields)` produces.
async fn page(
    client: &deadpool_postgres::Client,
    query: &IssueQuery,
    description: bool,
    cursor: Option<&str>,
    timezone: Tz,
) -> Result<Json, ViewError> {
    let fields = v2_fields();
    let extra: Vec<ExtraColumn> = if description {
        vec![ExtraColumn {
            key: "description_html",
            expression: Frag::raw("issues.description_html"),
        }]
    } else {
        Vec::new()
    };
    plane_query::global::page_with(client, query, &fields, &extra, cursor, timezone)
        .await
        .map_err(|error| ViewError::Unknown(error.message()))
}
