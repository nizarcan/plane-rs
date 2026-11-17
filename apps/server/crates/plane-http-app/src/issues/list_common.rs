// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Helpers shared by the issue read routes (`src/issues/list_*.rs`).
//!
//! Everything here is used by at least two handlers: the project/membership
//! lookup every view repeats, the `QueryError` → response mapping that keeps
//! `IssueViewSet`'s two error dialects apart, and the issue-detail row the
//! three "one issue" routes share.
//!
//! ## Where the SQL lives
//!
//! The statements with a fixed shape are declared here (next to their kind)
//! instead of `src/issues/queries.rs`, so the two issue agents' branches do not
//! collide on one file. They are `prepare_cached` exactly like a catalogued
//! statement (`DECISIONS.md` D1); listing them in
//! `src/issues/queries.rs::ALL_QUERIES` is the only missing step for
//! `tests/schema_check.rs` to prepare them too.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::plane_query;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_query::error::QueryError;
use plane_query::json::Json;
use plane_query::sql::Frag;
use plane_serde::Dialect;
use uuid::Uuid;

/// `ROLE.GUEST.value` (`plane/app/permissions/__init__.py`).
pub const ROLE_GUEST: i16 = 5;

// ---------------------------------------------------------------------------
// Errors and bodies
// ---------------------------------------------------------------------------

/// `QueryError` → the response Django would send, when the Python raises a
/// DRF exception or a bare `{"error": …}` body.
///
/// `plane_query` documents which of its variants are 400s and which are the
/// upstream 500s; this is the one place that turns that into HTTP.
pub fn query_error_response(error: &QueryError) -> Option<ViewResponse> {
    match error {
        QueryError::Api(api) => {
            // `ApiError::detail` is the exact body DRF renders, in Python's key
            // order — except for the plain case, where it is the message and
            // DRF's `APIException` wraps it in `{"detail": …}`.
            let detail = api.detail();
            let body = if api.code.is_none() && api.errors.is_none() {
                Json::object(vec![("detail", detail)])
            } else {
                detail
            };
            Some(rendered(api.status, &body))
        }
        QueryError::BadRequest(message) => Some(error_body(400, message)),
        // `django.core.exceptions.ValidationError` from a lookup value; the
        // family handler's body (`plane/app/views/base.py:216-220`) is the
        // `{"error": …}` dialect, not DRF's `{"detail": …}`.
        QueryError::Validation => Some(error_body(400, "Please provide valid detail")),
        _ => None,
    }
}

/// A `QueryError` as the handler's result: a 400 renders the body it would in
/// Django, anything else is the 500 the family handler produces.
pub fn map_query_error(error: QueryError) -> Result<ViewResponse, ViewError> {
    match query_error_response(&error) {
        Some(response) => Ok(response),
        None => Err(ViewError::Unknown(error.message())),
    }
}

/// The 500 a `QueryError` that Django lets escape produces. A 400 keeps its
/// message (the body itself needs [`query_error_response`]).
pub fn query_error(error: QueryError) -> ViewError {
    ViewError::Unknown(error.message())
}

/// `Response({"error": …}, status)` in DRF's JSON dialect.
///
/// Plane builds these bodies by hand (`plane/app/views/issue/base.py:71,393`),
/// so the key is `error`, not DRF's `detail`.
pub fn error_body(status: u16, message: &str) -> ViewResponse {
    rendered(
        status,
        &Json::object(vec![("error", Json::string(message))]),
    )
}

/// A finished `Json` document as the response body.
pub fn rendered(status: u16, body: &Json) -> ViewResponse {
    ViewResponse::raw(status, APPLICATION_JSON, body.to_drf())
}

/// A hand-built body rendered through DRF's JSON dialect, preserving key order.
pub fn body_of(entries: Vec<(&str, Json)>) -> Json {
    Json::object(entries)
}

/// A handler's `Ok` for a `Json` document.
pub fn json_response(body: Json) -> Result<ViewResponse, ViewError> {
    Ok(rendered(200, &body))
}

// ---------------------------------------------------------------------------
// Request plumbing
// ---------------------------------------------------------------------------

/// `DateTimeField.to_python` for a plain ORM lookup value
/// (`django/db/models/fields/__init__.py:1594-1624`), which is what the
/// `updated_at__gt` extra filter goes through.
///
/// Django 5.2 uses `datetime.fromisoformat` first and falls back to
/// `parse_datetime`; both accept a bare date, a `T` or space separator, an
/// optional fraction and an optional `Z`/`±HH:MM` offset. A value with an offset
/// is converted to UTC; a **naive** one is made aware by `get_prep_value` with
/// `timezone.get_default_timezone()` — `settings.TIME_ZONE` (`"UTC"`), not the
/// activated user timezone (`__init__.py:1660-1678`, the `RuntimeWarning`
/// branch). So a naive bound is UTC regardless of the caller. Anything else is
/// Django's `ValidationError`, i.e. the family handler's 400.
pub fn parse_django_datetime(raw: &str) -> Result<DateTime<Utc>, ViewError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(ViewError::Validation);
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Ok(parsed.with_timezone(&Utc));
    }
    // `datetime.fromisoformat` accepts a trailing lower-case `z` and a
    // `+HHMM`/`-HHMM` offset without a colon; normalise those two forms.
    let mut owned = text.to_owned();
    if let Some(stripped) = owned.strip_suffix('z') {
        owned = format!("{stripped}Z");
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&owned) {
        return Ok(parsed.with_timezone(&Utc));
    }
    // A naive value: date, date + time with `T` or space, optional seconds and
    // fraction.
    let naive_formats = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ];
    for format in naive_formats {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Ok(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0).ok_or(ViewError::Validation)?;
        return Ok(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    Err(ViewError::Validation)
}

/// A UUID URL kwarg. The routing table only matches Django's converter, so a
/// failure here is a bug rather than a client error, and Django's own
/// `UUIDField.to_python` 400 is unreachable for these routes.
pub fn kwarg_uuid(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    let raw = ctx.kwarg(name).ok_or(ViewError::Key)?;
    Uuid::parse_str(raw).map_err(|_| ViewError::Validation)
}

/// A required non-UUID URL kwarg.
pub fn kwarg_text<'a>(ctx: &'a ViewCtx<'a>, name: &str) -> Result<&'a str, ViewError> {
    ctx.kwarg(name).ok_or(ViewError::Key)
}

/// `request.user.user_timezone`, defaulting to `settings.TIME_ZONE` (UTC) the
/// way `TimezoneMixin`'s `deactivate()` leaves it.
pub fn user_timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// `request.user.id` on an authenticated route.
pub fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user.id().ok_or(ViewError::Key)
}

/// The request's connection (`common::client`, inlined so the call sites read
/// the same as Django's `Model.objects`).
pub fn client<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a Client, ViewError> {
    crate::common::client(ctx)
}

// ---------------------------------------------------------------------------
// Project and membership
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)` joined with
    /// the caller's `ProjectMember` row, which the issue views read separately:
    ///
    /// * `Project.objects.get(...)` — `plane/app/views/issue/base.py:290,393`;
    /// * `ProjectMember.objects.filter(...).exists()` — `:310,591,1173`.
    ///
    /// One statement instead of Django's two. `project_members` is `LEFT
    /// OUTER`: a caller who is not a member has already been rejected by
    /// `allow_permission`, but the views still ask for the row and treat a miss
    /// as "not a guest".
    ///
    /// The join to `workspaces` is Django's `workspace__slug` filter; the
    /// related model's manager is not consulted, so a soft-deleted workspace
    /// still matches (see the ledger's "suspected upstream bugs").
    PROJECT_WITH_MEMBERSHIP = "list_project_with_membership",
    "SELECT p.id, p.workspace_id, p.identifier, p.guest_view_all_features, \
     pm.role, pm.is_active \
     FROM projects p \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     LEFT OUTER JOIN project_members pm \
       ON pm.project_id = p.id AND pm.member_id = $3 AND pm.deleted_at IS NULL \
     WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL \
     LIMIT 1",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("guest_view_all_features", Bool),
        ("role", Int2),
        ("is_active", Bool),
    ]
}

/// The project and the caller's membership, as the issue views read them.
#[derive(Debug, Clone)]
pub struct RouteProject {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub identifier: String,
    pub guest_view_all_features: bool,
    /// `None` when the caller has no live `ProjectMember` row.
    pub role: Option<i16>,
    pub is_active: bool,
}

impl RouteProject {
    /// `ProjectMember.objects.filter(..., role=5, is_active=True).exists()`.
    pub fn is_guest(&self) -> bool {
        self.role == Some(ROLE_GUEST) && self.is_active
    }
}

/// [`PROJECT_WITH_MEMBERSHIP`] as a lookup that raises Django's
/// `DoesNotExist` (the family 404 body) on a miss.
pub async fn project_with_membership(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<RouteProject, ViewError> {
    let statement = client
        .prepare_cached(PROJECT_WITH_MEMBERSHIP.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id, &slug, &user_id])
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(RouteProject {
        id: row.get(0),
        workspace_id: row.get(1),
        identifier: row.get(2),
        guest_view_all_features: row.get(3),
        role: row.get(4),
        is_active: row.get::<_, Option<bool>>(5).unwrap_or(false),
    })
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=…, project_id=…,
    /// member=…, is_active=True).exists()` — the check `IssueDetailEndpoint`
    /// and `IssueDetailIdentifierEndpoint` run before their issue query.
    PROJECT_MEMBERSHIP_EXISTS = "list_project_membership_exists",
    "SELECT 1 FROM project_members \
     INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND project_members.project_id = $2 \
       AND project_members.member_id = $3 AND project_members.is_active \
       AND project_members.deleted_at IS NULL \
     LIMIT 1",
    [("exists", Int4)]
}

/// `ProjectMember.objects.filter(workspace__slug=…, project_id=…, member=…,
/// is_active=True).exists()`.
pub async fn is_project_member(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(PROJECT_MEMBERSHIP_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&slug, &project_id, &user_id])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

plane_query! {
    /// `ProjectMember.objects.filter(workspace__slug=…, project_id=…,
    /// member=…, role=5, is_active=True).exists()` — the guest restriction
    /// half of `IssueViewSet.list` (`plane/app/views/issue/base.py:310-319`)
    /// and `IssueDetailEndpoint` (`:1105-1112`).
    PROJECT_IS_RESTRICTED_GUEST = "list_project_is_restricted_guest",
    "SELECT 1 FROM project_members \
     INNER JOIN workspaces ON project_members.workspace_id = workspaces.id \
     WHERE workspaces.slug = $1 AND project_members.project_id = $2 \
       AND project_members.member_id = $3 AND project_members.role = 5 \
       AND project_members.is_active AND project_members.deleted_at IS NULL \
     LIMIT 1",
    [("exists", Int4)]
}

/// Whether the caller is a guest of this project (the role half only).
pub async fn is_guest_member(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(PROJECT_IS_RESTRICTED_GUEST.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&slug, &project_id, &user_id])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

// ---------------------------------------------------------------------------
// The list orchestration
// ---------------------------------------------------------------------------

/// Everything one issue-list request needs, in the order the Python does it.
pub struct ListRequest<'a> {
    pub query: &'a plane_drf::QueryDict,
    pub scope: plane_query::issue::query::IssueScope,
    pub timezone: Tz,
    /// `timezone.now().date()` for the relative date filters.
    pub now: DateTime<Utc>,
    /// A predicate every statement must also apply.
    ///
    /// The views add lookups that are not `IssueFilterSet` fields
    /// (`updated_at__gt` in `IssueViewSet.list`, `pk__in` and
    /// `state__deleted_at__isnull=True` in `IssueListEndpoint`). It is ANDed
    /// into the rich-filter predicate, so every statement `build()` feeds —
    /// page, counts, totals, group values — applies it.
    pub extra: Frag,
    /// The view's switches: which filter backends it declares, the annotation
    /// shape, `distinct`, and the `order_by` default.
    pub options: plane_query::ListOptions,
}

/// `plane_query::issue::run_with`, with [`ListRequest::extra`] folded in and
/// the view's own [`plane_query::ListOptions`].
///
/// `run_with` reproduces the Python's parse order (`IssueViewSet.list`):
/// filterset/JSON tree → group/sub-group equality → paginator parameters →
/// the compile-time filter errors. The extra predicate is the one thing the
/// crate has no view hook for; it becomes part of the scope filter (which
/// *replaces* the manager predicate, so the two are ANDed).
pub async fn run_issue_list(
    client: &Client,
    request: &ListRequest<'_>,
) -> Result<Json, QueryError> {
    let mut options = request.options.clone();
    options.scope_override = Some(request.scope.clone());
    if !request.extra.is_empty() {
        let base = options
            .scope
            .take()
            .map(|scope| scope.predicate)
            .unwrap_or_else(plane_query::issue::model::manager_predicate);
        options.scope = Some(plane_query::issue::query::IssueScopeFilter::new(
            base.and(request.extra.clone()),
        ));
    }
    let list_request = plane_query::issue::ListRequest {
        query: request.query,
        workspace_slug: &request.scope.workspace_slug,
        project_id: request.scope.project_id.unwrap_or_default(),
        timezone: request.timezone,
        now: request.now,
        user_id: request.scope.created_by,
        guest_view_all_features: true,
        is_guest: false,
    };
    plane_query::issue::run_with(client, &list_request, &options).await
}

/// Build the [`IssueQuery`](plane_query::issue::query::IssueQuery) without
/// running a paginator.
///
/// The endpoints that assemble their own statement (`IssueListEndpoint`,
/// `IssuePaginatedViewSet`, the archive list) need exactly the filters, ordering
/// and grouping `run_issue_list` parses; this is that half.
pub fn build_issue_query(
    request: &ListRequest<'_>,
) -> Result<plane_query::issue::query::IssueQuery, QueryError> {
    use plane_query::issue::filter::{FilterParams, LegacyFilters};
    use plane_query::issue::model::GroupField;
    use plane_query::issue::order::Ordering;
    use plane_query::issue::query::IssueQuery;
    use plane_query::issue::rich;

    // The parse order is the Python's: the filter tree first (its 400 wins over
    // a bad `order_by`), then the queryset's own `-created_at` default.
    let legacy = LegacyFilters::parse(&FilterParams::Get(request.query), request.now.date_naive())?;
    let rich = match request.options.filter_backends {
        plane_query::FilterBackends::LegacyAndComplex => {
            let rich = rich::parse_filterset(request.query, request.timezone)?;
            match rich::parse_complex_query(request.query, request.timezone)? {
                Some(complex) => Some(complex),
                None => rich,
            }
        }
        plane_query::FilterBackends::ComplexOnly => {
            rich::parse_complex_query(request.query, request.timezone)?
        }
    };
    let rich = combine_rich(rich, request.extra.clone());
    let ordering = Ordering::resolve(
        request
            .query
            .get_or("order_by", request.options.default_order_by),
    );
    let (group_by, sub_group_by) = if request.options.grouping {
        (
            request
                .query
                .get("group_by")
                .filter(|value| !value.is_empty())
                .and_then(GroupField::parse),
            request
                .query
                .get("sub_group_by")
                .filter(|value| !value.is_empty())
                .and_then(GroupField::parse),
        )
    } else {
        (None, None)
    };
    Ok(IssueQuery {
        scope: request.scope.clone(),
        filters: legacy,
        rich,
        ordering,
        group_by,
        sub_group_by,
        timezone: request.timezone,
    })
}

/// The parsing half of [`run_issue_list`], so a handler can enqueue its Celery
/// task at exactly the point the Python does.
///
/// `IssueViewSet.list` parses the legacy filters and the filterset *before*
/// `recent_visited_task.delay` and paginates *after* it
/// (`plane/app/views/issue/base.py:292-322`), so an invalid filter must not
/// enqueue the task while an invalid `per_page`/`cursor` must.
pub fn prevalidate_list_filters(
    query: &plane_drf::QueryDict,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Result<(), QueryError> {
    use plane_query::issue::filter::{FilterParams, LegacyFilters};
    use plane_query::issue::rich;

    // `issue_filters` itself cannot fail, but `queryset.filter(**filters)` is
    // built right after it and `DateField.get_prep_value` raises there — before
    // `recent_visited_task.delay` (`base.py:284,304`) — so a bad legacy date is
    // a 400 that must not enqueue the task.
    LegacyFilters::parse(&FilterParams::Get(query), now.date_naive())?;
    // `filter_queryset` runs before the task (`base.py:281-284`).
    if let Some(complex) = rich::parse_complex_query(query, timezone)? {
        drop(complex);
    }
    Ok(())
}

/// AND a predicate into a parsed filterset, keeping its joins.
pub fn combine_rich(
    rich: Option<plane_query::issue::rich::RichFilters>,
    extra: Frag,
) -> Option<plane_query::issue::rich::RichFilters> {
    if extra.is_empty() {
        return rich;
    }
    match rich {
        Some(mut rich) => {
            rich.predicate = rich.predicate.and(extra);
            Some(rich)
        }
        None => Some(plane_query::issue::rich::RichFilters {
            joins: Vec::new(),
            predicate: extra,
        }),
    }
}

// ---------------------------------------------------------------------------
// Small typed reads
// ---------------------------------------------------------------------------

/// One `(uuid, text)` pair, used by the issue-detail expansions.
#[derive(Debug, Clone)]
pub struct IdText {
    pub id: Uuid,
    pub text: String,
}

/// Read `(id, column)` pairs from a statement prepared on the fly.
pub async fn id_text_pairs(
    client: &Client,
    sql: &plane_query::sql::Sql,
) -> Result<Vec<IdText>, ViewError> {
    let rows = sql.query(client).await.map_err(ViewError::from)?;
    Ok(rows
        .iter()
        .map(|row| IdText {
            id: row.get(0),
            text: row.get(1),
        })
        .collect())
}

/// A `DateTime<Utc>` from a column that Django stores as UTC, or `None`.
pub fn opt_datetime(row: &tokio_postgres::Row, index: usize) -> Option<DateTime<Utc>> {
    row.get::<_, Option<DateTime<Utc>>>(index)
}

/// A `NaiveDate` from a `date` column, or `None`.
pub fn opt_date(row: &tokio_postgres::Row, index: usize) -> Option<NaiveDate> {
    row.get::<_, Option<NaiveDate>>(index)
}

/// Re-export so every handler renders with the same dialect.
pub fn to_drf(value: &impl serde::Serialize) -> Vec<u8> {
    plane_serde::to_vec(Dialect::Drf, value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(response: ViewResponse) -> String {
        match response {
            ViewResponse::Raw { body, .. } => String::from_utf8(body).expect("utf8"),
            ViewResponse::Json { .. } => panic!("expected a rendered body"),
        }
    }

    #[test]
    fn the_error_body_is_plane_s_dialect() {
        assert_eq!(
            body(error_body(400, "Issues are required")),
            r#"{"error":"Issues are required"}"#
        );
    }

    #[test]
    fn a_query_bad_request_keeps_django_s_body() {
        let error =
            QueryError::BadRequest("Group by and sub group by cannot have same parameters".into());
        let response = query_error_response(&error).expect("response");
        assert_eq!(response.status(), 400);
        assert_eq!(
            body(response),
            r#"{"error":"Group by and sub group by cannot have same parameters"}"#
        );
    }

    #[test]
    fn a_parse_error_is_drf_s_detail_body() {
        let error = QueryError::parse("Invalid cursor parameter.");
        let response = query_error_response(&error).expect("response");
        assert_eq!(response.status(), 400);
        assert_eq!(body(response), r#"{"detail":"Invalid cursor parameter."}"#);
    }

    #[test]
    fn an_upstream_failure_has_no_response() {
        assert!(query_error_response(&QueryError::unsupported("KeyError")).is_none());
    }

    /// `updated_at__gt` with a naive value: Django's `get_prep_value` attaches
    /// `settings.TIME_ZONE` (`UTC`), not the caller's timezone
    /// (`django/db/models/fields/__init__.py:1660-1678`).
    #[test]
    fn a_naive_updated_at_bound_is_utc() {
        assert_eq!(
            parse_django_datetime("2026-11-17T20:00:00")
                .expect("naive datetime")
                .to_rfc3339(),
            "2026-11-17T20:00:00+00:00"
        );
        assert_eq!(
            parse_django_datetime("2026-11-17")
                .expect("bare date")
                .to_rfc3339(),
            "2026-11-17T00:00:00+00:00"
        );
        // An explicit offset still wins, and is normalised to UTC.
        assert_eq!(
            parse_django_datetime("2026-11-18T01:30:00+05:30")
                .expect("aware datetime")
                .to_rfc3339(),
            "2026-11-17T20:00:00+00:00"
        );
    }
}
