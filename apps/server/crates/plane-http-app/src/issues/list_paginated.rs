// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/projects/{project_id}/v2/issues/` —
//! `IssuePaginatedViewSet.list` (`plane/app/views/issue/base.py:772-967`).
//!
//! The global paginator (`plane/utils/global_paginator.py`) with the user's
//! timezone applied to `created_at`/`updated_at`; `plane_query::global::page`
//! implements it. Two differences keep this handler from calling it directly:
//!
//! * `?description=true` appends `description_html` to `values(...)`
//!   (`:900-902`), a column `ValueField` does not have. The statement is
//!   composed here instead, so the page stays two round trips (count + page)
//!   rather than three.
//! * the guest restriction and `updated_at__gt` are plain ORM filters on both
//!   querysets (`:965-967`), passed as extra predicates.
//!
//! Everything else — the cursor format, the clamping of `page_size`, the
//! `next_cursor: null` on the last page, `page_count`, `total_pages`, the
//! envelope key order and the local-time rendering — is
//! `plane_query::global`'s.

use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::global::{PAGINATOR_MAX_LIMIT, PaginateCursor};
use plane_query::issue::model::ValueField;
use plane_query::issue::query::{IssueQuery, IssueScope};
use plane_query::issue::values;
use plane_query::json::Json;
use plane_query::sql::{Frag, Select, Value};

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

/// `paginate(base_queryset, queryset, cursor, on_result)` with the optional
/// `description_html` column appended after `sub_issues_count`.
async fn page(
    client: &deadpool_postgres::Client,
    query: &IssueQuery,
    description: bool,
    cursor: Option<&str>,
    timezone: Tz,
) -> Result<Json, ViewError> {
    let cursor_object = match cursor {
        None => PaginateCursor::DEFAULT,
        Some(text) => PaginateCursor::from_string(text)
            .map_err(|error| ViewError::Unknown(error.message()))?,
    };

    let base = query.filtered_base();
    let total_results = count(client, &base).await?;
    let page_size = cursor_object.current_page_size.min(PAGINATOR_MAX_LIMIT);
    if page_size <= 0 {
        return Err(ViewError::Unknown(
            "division by zero in total_pages (page size is not positive)".to_owned(),
        ));
    }
    let total_pages = if total_results == 0 {
        0
    } else {
        (total_results + page_size - 1) / page_size
    };
    let start_index = if cursor_object.current_page > 0 {
        cursor_object.current_page * page_size
    } else {
        0
    };
    let end_index = (start_index + page_size).min(total_results);

    let fields = v2_fields();
    let mut select = query.filtered_select();
    select.projection = values::projection_with(values::Projection::IssueListEndpoint, &fields);
    if description {
        select
            .projection
            .push(Frag::raw("issues.description_html AS \"description_html\""));
    }
    select.order_by = vec![Frag::raw("issues.updated_at ASC")];
    select.distinct = false;
    select.limit = Some((end_index - start_index).max(0));
    select.offset = Some(start_index);

    let rows = fetch(client, &select).await?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut value =
            values::decode(row, &fields).map_err(|error| ViewError::Unknown(error.message()))?;
        if description && let Json::Object(entries) = &mut value {
            entries.push((
                "description_html".to_owned(),
                Json::string(row.get::<_, String>(fields.len())),
            ));
        }
        results.push(shift_timezone(value, timezone));
    }

    let prev_cursor = format!("{page_size}:{}:0", cursor_object.current_page - 1);
    let current_cursor = format!("{page_size}:{}:0", cursor_object.current_page);
    let next_cursor = if end_index < total_results {
        Some(format!("{page_size}:{}:0", cursor_object.current_page + 1))
    } else {
        None
    };
    let prev_page_results = cursor_object.current_page > 0;
    let next_page_results = next_cursor.is_some();

    Ok(Json::object(vec![
        ("prev_cursor", Json::string(prev_cursor)),
        ("cursor", Json::string(current_cursor)),
        ("next_cursor", next_cursor.map_or(Json::Null, Json::string)),
        ("prev_page_results", Json::Bool(prev_page_results)),
        ("next_page_results", Json::Bool(next_page_results)),
        ("page_count", Json::Int(results.len() as i64)),
        ("total_results", Json::Int(total_results)),
        ("total_pages", Json::Int(total_pages)),
        ("results", Json::Array(results)),
    ]))
}

async fn count(client: &deadpool_postgres::Client, select: &Select) -> Result<i64, ViewError> {
    let distinct = if select.distinct {
        Some("issues.id")
    } else {
        None
    };
    let sql = select.count_sql(distinct, None);
    let statement = client.prepare(&sql.text).await.map_err(ViewError::from)?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let row = client
        .query_one(&statement, &params)
        .await
        .map_err(ViewError::from)?;
    row.try_get(0).map_err(ViewError::from)
}

async fn fetch(
    client: &deadpool_postgres::Client,
    select: &Select,
) -> Result<Vec<tokio_postgres::Row>, ViewError> {
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

/// `user_timezone_converter(results, ["created_at", "updated_at"], tz)`.
fn shift_timezone(value: Json, timezone: Tz) -> Json {
    let Json::Object(entries) = value else {
        return value;
    };
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| {
                if (key == "created_at" || key == "updated_at")
                    && let Json::DateTime(value) = value
                {
                    let zoned = value.with_timezone(&timezone);
                    return (key, Json::DateTimeZoned(zoned.fixed_offset()));
                }
                (key, value)
            })
            .collect(),
    )
}
