// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/analytic/base.py` — the workspace analytics endpoints and
//! the saved analytic views.
//!
//! | Route | Python |
//! | --- | --- |
//! | `GET …/analytics/` | `AnalyticsEndpoint.get` (`base.py:38-180`) |
//! | `GET …/analytic-view/` | `AnalyticViewViewset.list` (`base.py:183-193`) |
//! | `GET …/analytic-view/{pk}/` | `AnalyticViewViewset.retrieve` |
//! | `GET …/saved-analytic-view/{analytic_id}/` | `SavedAnalyticEndpoint.get` (`base.py:196-232`) |
//!
//! Both analytics endpoints answer the same envelope —
//! `{"total", "distribution"}`, and `/analytics/` adds `"extras"` with five
//! detail collections that default to `{}` (an object, not an array) when the
//! axis does not need them. The distribution itself is
//! [`crate::analytics::plot::build_graph_plot`].
//!
//! Round trips per request: `/analytics/` is Django's `count()`, one aggregate
//! and one per requested detail collection (0–5); `/saved-analytic-view/` is
//! the `AnalyticView` row plus one aggregate and one count.

use chrono::Utc;
use chrono_tz::Tz;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::analytic::AnalyticView;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::issue::filter::{FilterParams, LegacyFilters};
use plane_query::issue::model::Relation;
use plane_query::json::Json;
use plane_query::sql::{Frag, JoinKind};
use plane_serializers::PgJson;
use plane_serializers::generated::app_analytic::AnalyticViewSerializer;
use uuid::Uuid;

use crate::analytics::plot::{build_graph_plot, is_valid_field, is_valid_y_axis};
use crate::analytics::queries;
use crate::analytics::sql::{self, QueryBase};
use crate::analytics::stored;
use crate::common::{client, jobs, lookup, render, render_list};

/// `AnalyticsEndpoint.get` (`plane/app/views/analytic/base.py:38-180`).
pub fn analytics<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let timezone = user_timezone(ctx);
        let (x_axis, y_axis, segment) = match axes(ctx)? {
            Ok(axes) => axes,
            Err(response) => return Ok(response),
        };
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;

        let legacy = legacy_filters(ctx)?;
        let base = || {
            QueryBase::issue_objects()
                .workspace_slug(slug)
                .project_members(member_id, true)
                .legacy(&legacy, timezone)
        };

        // `queryset.count()` runs before `build_graph_plot` (`base.py:72-75`).
        let total = count(client, &base()).await?;
        let distribution =
            build_graph_plot(client, base(), x_axis, y_axis, segment, timezone).await?;
        let extras = details(client, slug, member_id, &legacy, x_axis, segment, timezone).await?;

        let body = Json::Object(vec![
            ("total".to_owned(), Json::Int(total)),
            ("distribution".to_owned(), distribution),
            ("extras".to_owned(), extras),
        ]);
        Ok(rendered(&body))
    })
}

/// `AnalyticViewViewset.list` (`plane/app/views/analytic/base.py:183-193`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::ANALYTIC_VIEWS_BY_WORKSPACE.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&slug])
            .await
            .map_err(ViewError::from)?;
        let mut views = Vec::with_capacity(rows.len());
        for row in &rows {
            views.push(AnalyticView::from_row(row, 0)?);
        }
        let serializers: Vec<_> = views.iter().map(serializer_for).collect();
        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let body = render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `AnalyticViewViewset.retrieve` (DRF's `get_object()` over the same
/// queryset, `plane/app/views/analytic/base.py:192-193`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::ANALYTIC_VIEW_IN_WORKSPACE.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(&statement, &[&slug, &pk])
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            // `get_object()`: `Http404("No AnalyticView matches the given query.")`.
            return not_found("AnalyticView");
        };
        let view = AnalyticView::from_row(&row, 0)?;
        let body = render(ctx, &serializer_for(&view))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `SavedAnalyticEndpoint.get` (`plane/app/views/analytic/base.py:196-232`).
pub fn saved_analytic<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let analytic_id = lookup::uuid_kwarg(
            "analytic_id",
            ctx.kwarg("analytic_id").ok_or(ViewError::Key)?,
        )?;
        let timezone = user_timezone(ctx);
        let member_id = jobs::user_id(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::ANALYTIC_VIEW_IN_WORKSPACE.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(&statement, &[&slug, &analytic_id])
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            // `AnalyticView.objects.get(...)` raises `DoesNotExist`, which
            // Plane's family handler renders as
            // `404 {"error": "The required object does not exist."}`.
            return Err(ViewError::ObjectDoesNotExist);
        };
        let view = AnalyticView::from_row(&row, 0)?;

        // `x_axis`/`y_axis` come from the *stored* `query_dict`; only the
        // segment is read from this request (`base.py:210-225`).
        let stored_query: serde_json::Value = serde_json::from_str(view.query_dict.as_str())
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let stored = |key: &str| -> Option<String> {
            stored_query
                .get(key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let x_axis = stored("x_axis").unwrap_or_default();
        let y_axis = stored("y_axis").unwrap_or_default();
        if x_axis.is_empty()
            || y_axis.is_empty()
            || !is_valid_field(&x_axis)
            || !is_valid_y_axis(&y_axis)
        {
            return Ok(error_body(
                400,
                "x-axis and y-axis dimensions are required and the values should be valid",
            ));
        }
        let segment = ctx.query().get("segment").filter(|value| !value.is_empty());
        if let Some(segment) = segment
            && (!is_valid_field(segment) || x_axis == segment)
        {
            return Ok(error_body(
                400,
                "Both segment and x axis cannot be same and segment should be valid",
            ));
        }

        // `filter = analytic_view.query` splatted into `.filter(**filter)`.
        let filters: serde_json::Value = serde_json::from_str(view.query.as_str())
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let mut base = QueryBase::issue_objects()
            .workspace_slug(slug)
            .project_members(member_id, true);
        stored::apply(&mut base, &filters, timezone)?;

        // `build_graph_plot` runs before `queryset.count()` here
        // (`base.py:227-228`).
        let distribution =
            build_graph_plot(client, base.clone(), &x_axis, &y_axis, segment, timezone).await?;
        let total = count(client, &base).await?;

        let body = Json::Object(vec![
            ("total".to_owned(), Json::Int(total)),
            ("distribution".to_owned(), distribution),
        ]);
        Ok(rendered(&body))
    })
}

/// The `x_axis`/`y_axis`/`segment` validation both analytics endpoints share
/// (`base.py:41-57`).
///
/// `Ok(Err(response))` is one of the view-built 400s; `Err` is a real error.
#[allow(clippy::type_complexity)]
fn axes<'a>(
    ctx: &'a ViewCtx<'a>,
) -> Result<Result<(&'a str, &'a str, Option<&'a str>), ViewResponse>, ViewError> {
    let query = ctx.query();
    let x_axis = query.get("x_axis").unwrap_or("");
    let y_axis = query.get("y_axis").unwrap_or("");
    // `request.GET.get("x_axis", False)`: an absent parameter and an empty one
    // are both falsy, and neither is in the valid list.
    if x_axis.is_empty() || y_axis.is_empty() || !is_valid_field(x_axis) || !is_valid_y_axis(y_axis)
    {
        return Ok(Err(error_body(
            400,
            "x-axis and y-axis dimensions are required and the values should be valid",
        )));
    }
    let segment = query.get("segment").filter(|value| !value.is_empty());
    if let Some(segment) = segment
        && (!is_valid_field(segment) || x_axis == segment)
    {
        return Ok(Err(error_body(
            400,
            "Both segment and x axis cannot be same and segment should be valid",
        )));
    }
    Ok(Ok((x_axis, y_axis, segment)))
}

/// `issue_filters(request.GET, "GET")` (`plane/utils/issue_filters.py:428-463`).
///
/// A malformed date reaches the ORM as a string and `DateField` raises
/// `ValidationError`, which Plane's family handler renders as
/// `400 {"error": "Please provide valid detail"}` — the same pre-check the
/// issue list does (`crate::issues::list_common::legacy_date_error`).
pub(crate) fn legacy_filters(ctx: &ViewCtx<'_>) -> Result<LegacyFilters, ViewError> {
    let query = ctx.query();
    if legacy_date_error(query) {
        return Err(ViewError::Validation);
    }
    LegacyFilters::parse(&FilterParams::Get(query), Utc::now().date_naive())
        .map_err(|error| ViewError::Unknown(error.message()))
}

/// Whether a legacy date lookup carries a value `DateField` rejects.
fn legacy_date_error(query: &plane_drf::QueryDict) -> bool {
    const KEYS: [&str; 5] = [
        "created_at",
        "updated_at",
        "completed_at",
        "start_date",
        "target_date",
    ];
    for key in KEYS {
        let Some(raw) = query.get(key) else { continue };
        for item in raw.split(',') {
            if item.is_empty() {
                continue;
            }
            let parts: Vec<&str> = item.split(';').collect();
            if parts.len() < 2 {
                continue;
            }
            if relative_date(parts[0]) {
                continue;
            }
            if !is_django_date(parts[0]) {
                return true;
            }
        }
    }
    false
}

/// `\d+_(weeks|months)`.
fn relative_date(value: &str) -> bool {
    value.rsplit_once('_').is_some_and(|(digits, unit)| {
        !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && matches!(unit, "weeks" | "months")
    })
}

/// `django.utils.dateparse.parse_date`: `YYYY-MM-DD` only.
fn is_django_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        && chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

/// The five `extras` collections (`base.py:77-165`).
///
/// Each is an empty object unless the axis needs it; then it is a list in the
/// order Django's `values()` projects. Django runs one statement each, in the
/// order the view builds them (state, assignee, label, cycle, module).
async fn details(
    client: &deadpool_postgres::Client,
    slug: &str,
    member_id: Uuid,
    legacy: &LegacyFilters,
    x_axis: &str,
    segment: Option<&str>,
    timezone: Tz,
) -> Result<Json, ViewError> {
    let wanted = |name: &str| x_axis == name || segment == Some(name);

    let issue_base = || {
        QueryBase::issue_objects()
            .workspace_slug(slug)
            .project_members(member_id, true)
            .legacy(legacy, timezone)
    };

    let state_details = if wanted("state_id") {
        sql_state_details(client, &issue_base()).await?
    } else {
        Json::Object(Vec::new())
    };
    let assignee_details = if wanted("assignees__id") {
        sql_assignee_details(client, &issue_base()).await?
    } else {
        Json::Object(Vec::new())
    };
    // Django passes the detail lookups as keyword arguments *after*
    // `**filters`, so a parameter that already set the same lookup is a
    // `TypeError: got multiple values for keyword argument …` — Plane's family
    // 500. `issue_filters` sets exactly these three keys whenever its
    // parameter is present (`plane/utils/issue_filters.py:158,331,346`).
    if (wanted("labels__id") && legacy.labels_join)
        || (wanted("issue_cycle__cycle_id") && legacy.cycle_join)
        || (wanted("issue_module__module_id") && legacy.module_join)
    {
        return Err(ViewError::Unknown(
            "TypeError: QuerySet.filter() got multiple values for keyword argument".to_owned(),
        ));
    }

    let label_details = if wanted("labels__id") {
        // `Issue.objects` — the plain manager, so no `issue_objects`
        // predicate (`base.py:89-98`).
        let base = QueryBase::issue_objects_off()
            .workspace_slug(slug)
            .project_members(member_id, true)
            .legacy(legacy, timezone);
        sql_label_details(client, &base).await?
    } else {
        Json::Object(Vec::new())
    };
    let cycle_details = if wanted("issue_cycle__cycle_id") {
        sql_cycle_details(client, &issue_base()).await?
    } else {
        Json::Object(Vec::new())
    };
    let module_details = if wanted("issue_module__module_id") {
        sql_module_details(client, &issue_base()).await?
    } else {
        Json::Object(Vec::new())
    };

    Ok(Json::Object(vec![
        ("state_details".to_owned(), state_details),
        ("assignee_details".to_owned(), assignee_details),
        ("label_details".to_owned(), label_details),
        ("cycle_details".to_owned(), cycle_details),
        ("module_details".to_owned(), module_details),
    ]))
}

async fn sql_state_details(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let projection = Frag::raw(
        "DISTINCT ON (issues.state_id) issues.state_id AS state_id, \
         states.name AS state__name, states.color AS state__color",
    );
    let rows = sql::fetch(client, &base.statement(projection, " ORDER BY 1 ASC")).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("state_id".to_owned(), read_uuid(row, 0)?),
            ("state__name".to_owned(), read_text(row, 1)?),
            ("state__color".to_owned(), read_text(row, 2)?),
        ]));
    }
    Ok(Json::Array(items))
}

async fn sql_assignee_details(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    base.require_relation(Relation::IssueAssignees, JoinKind::Left);
    sql::require_assignee_users(&mut base, JoinKind::Left);
    // `Q(assignees__avatar__isnull=False) | Q(assignees__avatar_asset__isnull=False)`
    // — Django keeps the two `LEFT OUTER` joins and filters in the `WHERE`
    // (`base.py:103-107`).
    base.and(Frag::raw(
        "assignee_user.avatar IS NOT NULL OR assignee_user.avatar_asset_id IS NOT NULL",
    ));
    let projection = format!(
        "DISTINCT ON (issue_assignees.assignee_id) {} AS assignees__avatar_url, \
         assignee_user.display_name AS assignees__display_name, \
         assignee_user.first_name AS assignees__first_name, \
         assignee_user.last_name AS assignees__last_name, \
         issue_assignees.assignee_id AS assignees__id",
        sql::avatar_url("assignee_user")
    );
    let rows = sql::fetch(
        client,
        &base.statement(Frag::raw(projection), " ORDER BY 5 ASC"),
    )
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("assignees__avatar_url".to_owned(), read_text(row, 0)?),
            ("assignees__display_name".to_owned(), read_text(row, 1)?),
            ("assignees__first_name".to_owned(), read_text(row, 2)?),
            ("assignees__last_name".to_owned(), read_text(row, 3)?),
            ("assignees__id".to_owned(), read_uuid(row, 4)?),
        ]));
    }
    Ok(Json::Array(items))
}

async fn sql_label_details(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    // `labels__id__isnull=False` and `label_issue__deleted_at__isnull=True`
    // promote both joins to `INNER` (`base.py:89-98`).
    base.require_relation(Relation::IssueLabels, JoinKind::Inner);
    sql::require_labels(&mut base);
    base.and(Frag::is_not_null("issue_labels.label_id"));
    base.and(Frag::is_null("issue_labels.deleted_at"));
    let projection = Frag::raw(
        "DISTINCT ON (issue_labels.label_id) issue_labels.label_id AS labels__id, \
         labels.color AS labels__color, labels.name AS labels__name",
    );
    let rows = sql::fetch(client, &base.statement(projection, " ORDER BY 1 ASC")).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("labels__id".to_owned(), read_uuid(row, 0)?),
            ("labels__color".to_owned(), read_text(row, 1)?),
            ("labels__name".to_owned(), read_text(row, 2)?),
        ]));
    }
    Ok(Json::Array(items))
}

async fn sql_cycle_details(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    base.require_relation(Relation::CycleIssues, JoinKind::Inner);
    sql::require_cycles(&mut base);
    base.and(Frag::is_not_null("cycle_issues.cycle_id"));
    base.and(Frag::is_null("cycle_issues.deleted_at"));
    let projection = Frag::raw(
        "DISTINCT ON (cycle_issues.cycle_id) cycle_issues.cycle_id AS issue_cycle__cycle_id, \
         cycles.name AS issue_cycle__cycle__name",
    );
    let rows = sql::fetch(client, &base.statement(projection, " ORDER BY 1 ASC")).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("issue_cycle__cycle_id".to_owned(), read_uuid(row, 0)?),
            ("issue_cycle__cycle__name".to_owned(), read_text(row, 1)?),
        ]));
    }
    Ok(Json::Array(items))
}

async fn sql_module_details(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<Json, ViewError> {
    let mut base = base.clone();
    base.require_relation(Relation::ModuleIssues, JoinKind::Inner);
    sql::require_modules(&mut base);
    base.and(Frag::is_not_null("module_issues.module_id"));
    base.and(Frag::is_null("module_issues.deleted_at"));
    let projection = Frag::raw(
        "DISTINCT ON (module_issues.module_id) module_issues.module_id AS issue_module__module_id, \
         modules.name AS issue_module__module__name",
    );
    let rows = sql::fetch(client, &base.statement(projection, " ORDER BY 1 ASC")).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(Json::Object(vec![
            ("issue_module__module_id".to_owned(), read_uuid(row, 0)?),
            ("issue_module__module__name".to_owned(), read_text(row, 1)?),
        ]));
    }
    Ok(Json::Array(items))
}

/// `SELECT COUNT(*)`.
pub(crate) async fn count(
    client: &deadpool_postgres::Client,
    base: &QueryBase,
) -> Result<i64, ViewError> {
    let rows = sql::fetch(client, &sql::count(base)).await?;
    match rows.first().map(|row| row.try_get::<_, i64>(0)) {
        Some(Ok(value)) => Ok(value),
        Some(Err(error)) => Err(ViewError::from(error)),
        None => Ok(0),
    }
}

pub(crate) fn read_uuid(row: &tokio_postgres::Row, index: usize) -> Result<Json, ViewError> {
    match row.try_get::<_, Option<Uuid>>(index) {
        Ok(Some(value)) => Ok(Json::Str(value.to_string())),
        Ok(None) => Ok(Json::Null),
        Err(error) => Err(ViewError::from(error)),
    }
}

pub(crate) fn read_text(row: &tokio_postgres::Row, index: usize) -> Result<Json, ViewError> {
    match row.try_get::<_, Option<String>>(index) {
        Ok(Some(value)) => Ok(Json::Str(value)),
        Ok(None) => Ok(Json::Null),
        Err(error) => Err(ViewError::from(error)),
    }
}

/// A one-key `{"message": …}` body built in the view (`advance.py:121,171,320`).
pub(crate) fn message_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::raw(
        status,
        APPLICATION_JSON,
        Json::object(vec![("message", Json::string(message))]).to_drf(),
    )
}

/// A one-key `{"error": …}` body built in the view.
pub(crate) fn error_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::raw(
        status,
        APPLICATION_JSON,
        Json::object(vec![("error", Json::string(message))]).to_drf(),
    )
}

/// `Http404("No <object> matches the given query.")` (`django/shortcuts.py`).
pub(crate) fn not_found(object: &str) -> Result<ViewResponse, ViewError> {
    Ok(ViewResponse::raw(
        404,
        APPLICATION_JSON,
        Json::object(vec![(
            "detail",
            Json::string(format!("No {object} matches the given query.")),
        )])
        .to_drf(),
    ))
}

/// `AnalyticViewSerializer(instance)`. Shared with the write handlers, which
/// render the in-memory instance after an INSERT/UPDATE (`write.rs`).
pub(super) fn serializer_for(view: &AnalyticView) -> AnalyticViewSerializer<'_> {
    AnalyticViewSerializer {
        id: view.id,
        created_at: view.created_at,
        updated_at: view.updated_at,
        deleted_at: view.deleted_at,
        name: &view.name,
        description: &view.description,
        query: Some(PgJson::from(&view.query)),
        query_dict: Some(PgJson::from(&view.query_dict)),
        created_by: view.created_by_id,
        updated_by: view.updated_by_id,
        workspace: view.workspace_id,
    }
}

/// `request.user.user_timezone`, defaulting to `settings.TIME_ZONE` (UTC).
pub(crate) fn user_timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// Render a hand-built body with the DRF dialect.
pub(crate) fn rendered(body: &Json) -> ViewResponse {
    ViewResponse::raw(200, APPLICATION_JSON, body.to_drf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_drf::QueryDict;

    fn query(raw: &str) -> QueryDict {
        QueryDict::parse(raw, None).expect("query")
    }

    #[test]
    fn a_legacy_date_is_rejected_the_way_datefield_rejects_it() {
        // `created_at=2026-08-01;after` is a date the ORM accepts.
        assert!(!legacy_date_error(&query("created_at=2026-08-01;after")));
        assert!(!legacy_date_error(&query(
            "created_at=60_weeks;after;fromnow"
        )));
        // `\d+_days` is not the relative pattern (`issue_filters.py:12`), so
        // the term before `after` reaches the ORM as a date string.
        assert!(legacy_date_error(&query(
            "created_at=60_days;after;fromnow"
        )));
        // A bare value is a `LIKE`, not a date lookup.
        assert!(!legacy_date_error(&query("created_at=2026-08")));
        // The term before the date reaches `__gte` as a string.
        assert!(legacy_date_error(&query("created_at=after;2026-10-02")));
        assert!(legacy_date_error(&query("start_date=nope;after")));
        assert!(!legacy_date_error(&query("priority=urgent")));
    }

    #[test]
    fn the_404_names_the_model() {
        let response = not_found("AnalyticView").expect("response");
        match response {
            ViewResponse::Raw { status, body, .. } => {
                assert_eq!(status, 404);
                assert_eq!(
                    String::from_utf8(body).expect("utf8"),
                    r#"{"detail":"No AnalyticView matches the given query."}"#
                );
            }
            _ => panic!("expected a rendered body"),
        }
    }

    #[test]
    fn the_two_error_bodies_use_their_python_key() {
        let response = message_body(400, "Invalid tab");
        match response {
            ViewResponse::Raw { status, body, .. } => {
                assert_eq!(status, 400);
                assert_eq!(
                    String::from_utf8(body).expect("utf8"),
                    r#"{"message":"Invalid tab"}"#
                );
            }
            _ => panic!("expected a rendered body"),
        }
        let response = error_body(400, "x");
        match response {
            ViewResponse::Raw { body, .. } => {
                assert_eq!(String::from_utf8(body).expect("utf8"), r#"{"error":"x"}"#)
            }
            _ => panic!("expected a rendered body"),
        }
    }
}
