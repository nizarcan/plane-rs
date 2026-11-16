// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.intake.base.IntakeWorkItemDescriptionVersionEndpoint.get`
//! (`plane/app/views/intake/base.py:574-642`).
//!
//! Two routes, one method: `/intake-work-items/{work_item_id}/description-versions/`
//! (a page) and `…/{pk}/` (one row).
//!
//! The list uses `plane/utils/global_paginator.py`'s `paginate`, **not**
//! `BasePaginator`: the cursor is `"<page_size>:<page>:<offset>"`, an
//! out-of-range page size is clamped, a malformed cursor is a `ValueError`
//! no view catches (500), `next_cursor` is `null` on the last page, and the
//! nine-key envelope has no `grouped_by`/`extra_stats` keys at all. The rows
//! are `.values(*required_fields)` dictionaries, so their keys are the ten
//! aliases the view asked for and `user_timezone_converter` shifts only
//! `created_at`/`updated_at` — `last_saved_at` stays UTC.
//!
//! `page_count` is `len(paginated_data)`, the number of rows on this page, and
//! `total_pages` is `ceil(total_results / page_size)`.

use chrono::{DateTime, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::models::issue::IssueDescriptionVersion;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::global::PaginateCursor;
use plane_query::json::Json;
use plane_serializers::PgJson;
use plane_serializers::generated::app_issue::IssueDescriptionVersionDetailSerializer;
use uuid::Uuid;

use super::queries;
use super::render::plain_context;
use crate::issues::lists::list_common::{client, error_body, kwarg_text, kwarg_uuid, user_id};

/// Register the two routes of the view (one handler, `pk` optional).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.intake.base.IntakeWorkItemDescriptionVersionEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `IntakeWorkItemDescriptionVersionEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let work_item_id = kwarg_uuid(ctx, "work_item_id")?;
        let pk = match ctx.kwarg("pk") {
            Some(_) => Some(kwarg_uuid(ctx, "pk")?),
            None => None,
        };
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Project.objects.get(pk=project_id)` and the guest check
        // (`base.py:585,588-602`).
        let project_statement = client
            .prepare_cached(queries::PROJECT_WITH_GUEST.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(project_row) = client
            .query_opt(&project_statement, &[&project_id, &user, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all_features: bool = project_row.get(0);
        let is_restricted_guest: bool = project_row.get(1);

        // `Issue.objects.get(workspace__slug=…, project_id=…, pk=work_item_id)`.
        let issue_statement = client
            .prepare_cached(queries::ISSUE_FOR_VERSIONS.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(issue_row) = client
            .query_opt(&issue_statement, &[&work_item_id, &project_id, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let issue_created_by: Option<Uuid> = issue_row.get(1);
        if is_restricted_guest && !guest_view_all_features && issue_created_by != Some(user) {
            return Ok(error_body(403, "You are not allowed to view this issue"));
        }

        if let Some(pk) = pk {
            return detail(ctx, client, project_id, work_item_id, pk, slug).await;
        }
        page(ctx, client, project_id, work_item_id, slug).await
    })
}

/// The `if pk:` branch: one `IssueDescriptionVersionDetailSerializer` row.
async fn detail(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
    slug: &str,
) -> Result<ViewResponse, ViewError> {
    let statement = client
        .prepare_cached(queries::DESCRIPTION_VERSION_DETAIL.sql)
        .await
        .map_err(ViewError::from)?;
    let Some(row) = client
        .query_opt(&statement, &[&issue_id, &pk, &project_id, &slug])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let version = IssueDescriptionVersion::from_row(&row, 0)?;
    let serializer = IssueDescriptionVersionDetailSerializer {
        id: version.id,
        workspace: version.workspace_id,
        project: version.project_id,
        issue: version.issue_id,
        description_binary: version.description_binary.as_deref(),
        description_html: &version.description_html,
        description_stripped: version.description_stripped.as_deref(),
        description_json: Some(PgJson::from(&version.description_json)),
        last_saved_at: version.last_saved_at,
        owned_by: version.owned_by_id,
        created_at: version.created_at,
        updated_at: version.updated_at,
        created_by: version.created_by_id,
        updated_by: version.updated_by_id,
    };
    let context = plain_context(ctx);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        plane_serializers::render(&serializer, &context)?,
    ))
}

/// The paginated branch.
async fn page(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    issue_id: Uuid,
    slug: &str,
) -> Result<ViewResponse, ViewError> {
    // `cursor = request.GET.get("cursor", None)`; `?cursor=` is the empty
    // string and therefore goes through `from_string`, not the default.
    let cursor_object = match ctx.query().get("cursor") {
        None => PaginateCursor::DEFAULT,
        Some(text) => PaginateCursor::from_string(text)
            .map_err(|error| ViewError::Unknown(error.message()))?,
    };

    let count_statement = client
        .prepare_cached(queries::DESCRIPTION_VERSION_COUNT.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_one(&count_statement, &[&issue_id, &project_id, &slug])
        .await
        .map_err(ViewError::from)?;
    let total_results: i64 = row.get(0);

    let page_size = cursor_object
        .current_page_size
        .min(plane_query::global::PAGINATOR_MAX_LIMIT);
    if page_size <= 0 {
        // `math.ceil(total / 0)` is Python's `ZeroDivisionError`: a 500.
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
    let limit = (end_index - start_index).max(0);

    let statement = client
        .prepare_cached(queries::DESCRIPTION_VERSION_PAGE.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(
            &statement,
            &[&issue_id, &project_id, &slug, &limit, &start_index],
        )
        .await
        .map_err(ViewError::from)?;

    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let results: Vec<Json> = rows
        .iter()
        .map(|row| {
            Json::object(vec![
                ("id", Json::uuid(row.get(0))),
                ("workspace", Json::uuid(row.get(1))),
                ("project", Json::uuid(row.get(2))),
                ("issue", Json::uuid(row.get(3))),
                // `user_timezone_converter` only rewrites the last two fields
                // (`plane/utils/timezone_converter.py:17-42`).
                (
                    "last_saved_at",
                    Json::DateTime(row.get::<_, DateTime<Utc>>(4)),
                ),
                ("owned_by", Json::uuid(row.get(5))),
                (
                    "created_at",
                    Json::DateTimeZoned(
                        row.get::<_, DateTime<Utc>>(6)
                            .with_timezone(&timezone)
                            .fixed_offset(),
                    ),
                ),
                (
                    "updated_at",
                    Json::DateTimeZoned(
                        row.get::<_, DateTime<Utc>>(7)
                            .with_timezone(&timezone)
                            .fixed_offset(),
                    ),
                ),
                ("created_by", Json::opt_uuid(row.get(8))),
                ("updated_by", Json::opt_uuid(row.get(9))),
            ])
        })
        .collect();

    let next_cursor = if end_index < total_results {
        Some(format!("{page_size}:{}:0", cursor_object.current_page + 1))
    } else {
        None
    };
    let next_page_results = next_cursor.is_some();
    let body = Json::object(vec![
        (
            "prev_cursor",
            Json::string(format!("{page_size}:{}:0", cursor_object.current_page - 1)),
        ),
        (
            "cursor",
            Json::string(format!("{page_size}:{}:0", cursor_object.current_page)),
        ),
        ("next_cursor", next_cursor.map_or(Json::Null, Json::string)),
        (
            "prev_page_results",
            Json::Bool(cursor_object.current_page > 0),
        ),
        ("next_page_results", Json::Bool(next_page_results)),
        ("page_count", Json::Int(results.len() as i64)),
        ("total_results", Json::Int(total_results)),
        ("total_pages", Json::Int(total_pages)),
        ("results", Json::Array(results)),
    ]);
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf()))
}
