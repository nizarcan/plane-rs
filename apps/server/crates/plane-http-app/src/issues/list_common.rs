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
/// optional fraction and an optional `Z`/`±HH:MM` offset. A naive value is
/// made aware in the **active** timezone (`TimezoneMixin` activated the user's
/// profile timezone), a value with an offset is converted to UTC. Anything else
/// is Django's `ValidationError`, i.e. the family handler's 400.
pub fn parse_django_datetime(raw: &str, timezone: Tz) -> Result<DateTime<Utc>, ViewError> {
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
            return match chrono::TimeZone::from_local_datetime(&timezone, &naive).earliest() {
                Some(value) => Ok(value.with_timezone(&Utc)),
                None => Err(ViewError::Validation),
            };
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0).ok_or(ViewError::Validation)?;
        return match chrono::TimeZone::from_local_datetime(&timezone, &naive).earliest() {
            Some(value) => Ok(value.with_timezone(&Utc)),
            None => Err(ViewError::Validation),
        };
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
    /// `plane_query::issue::run` has no hook for the extra lookups two views
    /// add (`updated_at__gt` in `IssueViewSet.list`, `pk__in` and
    /// `state__deleted_at__isnull=True` in `IssueListEndpoint`). It is ANDed
    /// into the rich-filter predicate, so every statement `build()` feeds —
    /// page, counts, totals, group values — applies it.
    pub extra: Frag,
}

/// `plane_query::issue::run`, plus [`ListRequest::extra`].
///
/// The only difference from the crate's `run` is the predicate hook; the
/// parsing order is the Python's (`IssueViewSet.list`):
/// legacy `issue_filters` → `filter_queryset` → ordering → grouper.
pub async fn run_issue_list(
    client: &Client,
    request: &ListRequest<'_>,
) -> Result<Json, QueryError> {
    use plane_query::issue::filter::{FilterParams, LegacyFilters};
    use plane_query::issue::model::GroupField;
    use plane_query::issue::order::Ordering;
    use plane_query::issue::query::{GroupPlan, IssueQuery};
    use plane_query::issue::rich;
    use plane_query::paginator::{PageQuery, issue_list_page};

    let page_query = request.query;
    let group_by_param = page_query.get("group_by").filter(|value| !value.is_empty());
    let sub_group_by_param = page_query
        .get("sub_group_by")
        .filter(|value| !value.is_empty());
    let page = PageQuery::parse(page_query, group_by_param, sub_group_by_param)?;

    let legacy = LegacyFilters::parse(&FilterParams::Get(page_query), request.now.date_naive())?;
    // Only `?filters=` is applied. `IssueViewSet.filter_backends` is
    // `(ComplexFilterBackend,)` (`plane/app/views/issue/base.py:274`), so the
    // `IssueFilterSet` is never run over the query parameters — it only
    // validates the field names of the JSON tree. `plane_query`'s `run` applies
    // `parse_filterset` as well, which turns e.g. `priority=urgent,high` into a
    // 400 that Django answers with the legacy filter's rows.
    let rich = rich::parse_complex_query(page_query, request.timezone)?;
    let rich = combine_rich(rich, request.extra.clone());

    let ordering = Ordering::resolve(page_query.get_or("order_by", "-created_at"));
    let group_by = group_by_param.and_then(GroupField::parse);
    let sub_group_by = sub_group_by_param.and_then(GroupField::parse);

    if group_by.is_some() && group_by == sub_group_by {
        // `plane/app/views/issue/base.py:327`: a bare `{"error": …}` body.
        return Err(QueryError::BadRequest(
            "Group by and sub group by cannot have same parameters".to_owned(),
        ));
    }

    let query = IssueQuery {
        scope: request.scope.clone(),
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

    let mut body = issue_list_page(client, &query, plan.as_ref(), &page).await?;
    if let Some(plan) = plan.as_ref() {
        fix_multi_group_arrays(client, &query, plan, &page, &mut body).await?;
    }
    Ok(body)
}

/// Rewrite the `assignee_ids` / `label_ids` / `module_ids` arrays of an
/// m2m-grouped response into CPython's `list(set(...))` order.
///
/// `__query_multi_grouper` (`plane/utils/paginator.py:601-618`) replaces the
/// issue's id array with `list(set(map(str, group_values)))`, inserted in page
/// row order. `plane-query` sorts those arrays (its golden corpus normalises
/// the difference), so the port re-derives the order here: it re-runs the same
/// page statement (one extra statement, only for `group_by`/`sub_group_by` on
/// `labels__id`, `assignees__id` or `issue_module__module_id`) and rebuilds
/// each issue's list in row order.
async fn fix_multi_group_arrays(
    client: &Client,
    query: &plane_query::issue::query::IssueQuery,
    plan: &plane_query::issue::query::GroupPlan,
    page: &plane_query::paginator::PageQuery,
    body: &mut Json,
) -> Result<(), QueryError> {
    use plane_query::issue::model::{GroupField, ValueField};

    let mapped: Vec<(GroupField, &'static str)> = [Some(plan.group_by), plan.sub_group_by]
        .into_iter()
        .flatten()
        .filter_map(|field| field.mapper().map(|mapper| (field, mapper)))
        .collect();
    if mapped.is_empty() {
        return Ok(());
    }

    let limit = i64::from(page.per_page.min(plane_query::paginator::MAX_LIMIT));
    let offset = page.cursor.offset * limit;
    let fields = ValueField::project(Some(plan.group_by), plan.sub_group_by);
    let sql = query.grouped_page(&fields, limit, offset, true).into_sql();
    let statement = client.prepare(&sql.text).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let rows = client.query(&statement, &params).await?;

    let id_index = fields
        .iter()
        .position(|field| *field == ValueField::Id)
        .unwrap_or(0);
    // issue id → mapper → group values in page order.
    #[allow(clippy::type_complexity)]
    let mut per_issue: Vec<(String, Vec<(&'static str, Vec<String>)>)> = Vec::new();
    for row in &rows {
        let id: uuid::Uuid = row.try_get(id_index)?;
        let id = id.to_string();
        let issue_index = match per_issue.iter().position(|(existing, _)| *existing == id) {
            Some(index) => index,
            None => {
                per_issue.push((id, Vec::new()));
                per_issue.len() - 1
            }
        };
        for (field, mapper) in &mapped {
            let Some(index) = fields
                .iter()
                .position(|candidate| *candidate == ValueField::Group(*field))
            else {
                continue;
            };
            let value = plane_query::issue::query::render_group_value_at(row, index, *field)?;
            let arrays = &mut per_issue[issue_index].1;
            let slot = match arrays.iter().position(|(name, _)| name == mapper) {
                Some(slot) => slot,
                None => {
                    arrays.push((mapper, Vec::new()));
                    arrays.len() - 1
                }
            };
            if !arrays[slot].1.contains(&value) {
                arrays[slot].1.push(value);
            }
        }
    }

    let mut replacements: Vec<(String, &'static str, Json)> = Vec::new();
    for (id, arrays) in &per_issue {
        for (mapper, values) in arrays {
            if values.iter().any(|value| value == "None") {
                replacements.push((id.clone(), mapper, Json::Array(Vec::new())));
            } else {
                let order = super::list_pyset::python_set_order(values);
                replacements.push((
                    id.clone(),
                    mapper,
                    Json::Array(order.into_iter().map(Json::Str).collect()),
                ));
            }
        }
    }

    apply_arrays(body, &replacements);
    Ok(())
}

/// Replace `row[mapper]` for every object carrying `id`, recursively.
fn apply_arrays(value: &mut Json, replacements: &[(String, &'static str, Json)]) {
    match value {
        Json::Array(items) => {
            for item in items {
                apply_arrays(item, replacements);
            }
        }
        Json::Object(entries) => {
            let id =
                entries
                    .iter()
                    .find(|(key, _)| key == "id")
                    .and_then(|(_, value)| match value {
                        Json::Str(id) => Some(id.clone()),
                        _ => None,
                    });
            if let Some(id) = id {
                for (key, slot) in entries.iter_mut() {
                    if let Some((_, _, replacement)) = replacements
                        .iter()
                        .find(|(target, mapper, _)| *target == id && *mapper == key)
                    {
                        *slot = replacement.clone();
                    }
                }
            }
            for (_, child) in entries.iter_mut() {
                apply_arrays(child, replacements);
            }
        }
        _ => {}
    }
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

    let legacy = LegacyFilters::parse(&FilterParams::Get(request.query), request.now.date_naive())?;
    // Only `?filters=`; see the note in `run_issue_list`.
    let rich = rich::parse_complex_query(request.query, request.timezone)?;
    let rich = combine_rich(rich, request.extra.clone());
    let ordering = Ordering::resolve(request.query.get_or("order_by", "-created_at"));
    let group_by = request
        .query
        .get("group_by")
        .filter(|value| !value.is_empty())
        .and_then(GroupField::parse);
    let sub_group_by = request
        .query
        .get("sub_group_by")
        .filter(|value| !value.is_empty())
        .and_then(GroupField::parse);
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

    LegacyFilters::parse(&FilterParams::Get(query), now.date_naive())?;
    if let Some(complex) = rich::parse_complex_query(query, timezone)? {
        drop(complex);
    }
    Ok(())
}

/// The date parameters `issue_filters` feeds to `date_filter`
/// (`plane/utils/issue_filters.py:63-81,198-296`).
const LEGACY_DATE_KEYS: [&str; 5] = [
    "created_at",
    "updated_at",
    "completed_at",
    "start_date",
    "target_date",
];

/// The 400 Django raises for a legacy date lookup whose first segment is not a
/// date.
///
/// `date_filter` writes `{term}__gte = date_query[0]` whenever the query is not
/// a relative range, so `start_date=after;2026-10-02` (the term before the
/// date) reaches the ORM as the string `"after"`; `DateField` raises
/// `ValidationError` when the queryset compiles, and the family handler renders
/// `{"error": "Please provide valid detail"}` (`plane/app/views/base.py:216-220`).
///
/// `plane-query` refuses the same input as an upstream 500 because its golden
/// corpus never contained the inverted form. Detecting it here keeps Django's
/// 400 without editing the query crate.
pub fn legacy_date_error(query: &plane_drf::QueryDict) -> Option<ViewResponse> {
    for key in LEGACY_DATE_KEYS {
        let Some(raw) = query.get(key) else { continue };
        for item in raw.split(',') {
            if item.is_empty() {
                continue;
            }
            let parts: Vec<&str> = item.split(';').collect();
            if parts.len() < 2 {
                continue;
            }
            // `\d+_(weeks|months)` is the relative form, which never reaches
            // the ORM as a date string.
            let relative = parts[0].rsplit_once('_').is_some_and(|(digits, unit)| {
                !digits.is_empty()
                    && digits.bytes().all(|byte| byte.is_ascii_digit())
                    && matches!(unit, "weeks" | "months")
            });
            if relative {
                continue;
            }
            if !is_django_date(parts[0]) {
                return Some(error_body(400, "Please provide valid detail"));
            }
        }
    }
    None
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

async fn fetch_group_values(
    client: &Client,
    query: &plane_query::issue::query::IssueQuery,
    field: plane_query::issue::model::GroupField,
) -> Result<Vec<String>, QueryError> {
    use plane_query::issue::group_values::{self, GroupValues};

    let filtered = query.filtered_base();
    let plan = group_values::values(&query.group_value_request(field, &filtered))?;
    match plan {
        GroupValues::Static(values) => Ok(values),
        GroupValues::Query { sql, append_none } => {
            let statement = client.prepare(&sql.text).await?;
            let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
                .params
                .iter()
                .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
                .collect();
            let rows = client.query(&statement, &params).await?;
            let mut values = Vec::with_capacity(rows.len() + 1);
            for row in &rows {
                values.push(plane_query::issue::query::render_group_value(row, field)?);
            }
            if append_none {
                values.push("None".to_owned());
            }
            let mut seen = std::collections::HashSet::new();
            values.retain(|value| seen.insert(value.clone()));
            Ok(values)
        }
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
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<IdText>, ViewError> {
    let statement = client.prepare(sql).await.map_err(ViewError::from)?;
    let rows = client
        .query(&statement, params)
        .await
        .map_err(ViewError::from)?;
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
}
