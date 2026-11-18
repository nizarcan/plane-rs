// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Work-item search: `GET /api/v1/workspaces/{slug}/issues/search/` (315) and
//! `GET /api/v1/workspaces/{slug}/work-items/search/` (327).
//!
//! Python: `plane/api/views/issue.py:2420-2502`
//! (`plane.api.views.issue.IssueSearchEndpoint`, a `BaseAPIView` with
//! `use_read_replica = True` and no `permission_classes`, so every authenticated
//! workspace member reaches it).
//!
//! ## What the view does
//!
//! 1. `search` absent or empty → `{"issues": []}` (200) without touching the
//!    database (`:2459-2460`). `limit` defaults to 10.
//! 2. It builds one `Q` object (`:2462-2472`) with `name__icontains`,
//!    `project__identifier__icontains` and, for every `\b\d+\b` run in the
//!    query, `sequence_id=<run>`. A numeric run that does not fit an `int4`
//!    can never match an `integer` column, so it is dropped (Django binds it
//!    and Postgres compares an int8; the row set is the same).
//! 3. `Exists(visible_issue_q(user))` per row (`:2477-2486`): a row is visible
//!    when the caller is an active project member above `GUEST`, or a `GUEST`
//!    on a project with `guest_view_all_features`, or a `GUEST` on a project
//!    without it *and* the row's creator. This is a per-row `EXISTS`, not a
//!    join, so the result set is the same as the app surface's list predicate
//!    but the SQL shape is not.
//! 4. `workspace_search == "false"` **and** a `project_id` narrows to that
//!    project (`:2489-2490`); any other `workspace_search` value is
//!    workspace-wide. The comparison is string equality, so
//!    `workspace_search=False` is *not* false.
//! 5. `Issue.issue_objects` adds `deleted_at IS NULL`, no triage state, not
//!    archived, project not archived and not a draft
//!    (`plane/db/models/issue.py:92-101`); `project__archived_at__isnull=True`
//!    is repeated by the view and collapses into the same predicate.
//! 6. `.distinct().values(...)[: int(limit)]`.
//!
//! ## Ported as-is
//!
//! * `int(limit)` is Python's `int()`: surrounding ASCII whitespace, a sign and
//!   single underscores between digits are accepted; anything else is a
//!   `ValueError` and therefore a 500 (`:2500`). A **negative** result reaches
//!   the slice as `[: -n]` and Django raises
//!   `AssertionError("Negative indexing is not supported.")`, also a 500.
//! * `limit` greater than the result set is just the whole set; there is no
//!   upper bound.
//! * The response is the `.values()` row list, so keys are the `values()`
//!   order and `sequence_id` is an int. Unlike the paginated lists, this route
//!   has no throttle-budget cost beyond the API-key throttle.

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::sql::{Frag, Value};
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use plane_http_app::common::RequestRender;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.issue.IssueSearchEndpoint")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// One `.values("name", "id", "sequence_id", "project__identifier",
/// "project_id", "workspace__slug")` row, in that key order.
#[derive(Debug)]
struct SearchRow {
    name: String,
    id: Uuid,
    sequence_id: i32,
    project_identifier: String,
    project_id: Uuid,
    workspace_slug: String,
}

impl DrfSerialize for SearchRow {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field("name", &self.name)?;
        obj.field("id", &plane_serializers::DrfUuid(self.id))?;
        obj.field("sequence_id", &self.sequence_id)?;
        obj.field("project__identifier", &self.project_identifier)?;
        obj.field("project_id", &plane_serializers::DrfUuid(self.project_id))?;
        obj.field("workspace__slug", &self.workspace_slug)?;
        obj.end();
        Ok(())
    }
}

/// `Response({"issues": issue_results})` (`issue.py:2502`).
#[derive(Debug)]
struct SearchResponse<'a> {
    rows: &'a [SearchRow],
}

impl DrfSerialize for SearchResponse<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_nested_list("issues", ctx, self.rows)?;
        obj.end();
        Ok(())
    }
}

/// `IssueSearchEndpoint.get` (`plane/api/views/issue.py:2448-2502`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;

        // `request.query_params.get("search", False)` (`:2454`).
        let Some(query) = ctx.query().get("search").filter(|value| !value.is_empty()) else {
            return empty_response(ctx);
        };
        let limit = python_int(ctx.query().get_or("limit", "10"))?;
        let workspace_search = ctx.query().get_or("workspace_search", "false").to_owned();
        let project_id = ctx
            .query()
            .get("project_id")
            .filter(|value| !value.is_empty())
            .map(str::to_owned);

        // `[: int(limit)]` with a negative bound is an `AssertionError`
        // (`django/db/models/query.py:340-352`), i.e. a 500.
        if limit < 0 {
            return Err(ViewError::Unknown(
                "AssertionError: Negative indexing is not supported.".to_owned(),
            ));
        }

        let client = plane_http_app::common::client(ctx)?;
        let mut sql = Frag::raw(
            "SELECT DISTINCT i.name, i.id, i.sequence_id, pr.identifier, \
             i.project_id, ws.slug, i.created_at \
             FROM issues i \
             INNER JOIN projects pr ON i.project_id = pr.id \
             INNER JOIN workspaces ws ON i.workspace_id = ws.id \
             LEFT OUTER JOIN states s ON i.state_id = s.id \
             WHERE i.deleted_at IS NULL \
               AND NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL) \
               AND NOT (i.archived_at IS NOT NULL) \
               AND NOT (pr.archived_at IS NOT NULL) \
               AND NOT (i.is_draft) \
               AND ws.slug = ",
        );
        sql.push(Frag::bind(Value::Text(slug.clone())));

        // `:2462-2472`: `Q(name__icontains) | Q(project__identifier__icontains)
        // | Q(sequence_id=<run>)…`.
        sql.push_raw(" AND (");
        sql.push(Frag::icontains("i.name", query));
        sql.push_raw(" OR ");
        sql.push(Frag::icontains("pr.identifier", query));
        for term in sequence_terms(query) {
            if let Ok(term) = i32::try_from(term) {
                sql.push_raw(" OR i.sequence_id = ");
                sql.push(Frag::bind(Value::Int(term)));
            }
        }
        sql.push_raw(")");

        // `:2477-2486`: `filter(Exists(visible_issue_subquery))` where the
        // subquery is the three-branch `visible_issue_q(user)` on the row.
        sql.push_raw(
            " AND EXISTS (SELECT 1 FROM issues vi \
               WHERE vi.id = i.id AND vi.deleted_at IS NULL \
                 AND (EXISTS (SELECT 1 FROM project_members pm \
                        WHERE pm.project_id = vi.project_id AND pm.member_id = ",
        );
        sql.push(Frag::bind(Value::Uuid(user)));
        sql.push_raw(
            " AND pm.is_active AND pm.deleted_at IS NULL AND pm.role > 5) \
                   OR EXISTS (SELECT 1 FROM project_members pm \
                        INNER JOIN projects vp ON vp.id = pm.project_id \
                        WHERE pm.project_id = vi.project_id AND pm.member_id = ",
        );
        sql.push(Frag::bind(Value::Uuid(user)));
        sql.push_raw(
            " AND pm.is_active AND pm.deleted_at IS NULL AND pm.role = 5 \
                          AND vp.guest_view_all_features) \
                   OR EXISTS (SELECT 1 FROM project_members pm \
                        INNER JOIN projects vp ON vp.id = pm.project_id \
                        WHERE pm.project_id = vi.project_id AND pm.member_id = ",
        );
        sql.push(Frag::bind(Value::Uuid(user)));
        sql.push_raw(
            " AND pm.is_active AND pm.deleted_at IS NULL AND pm.role = 5 \
                          AND NOT vp.guest_view_all_features \
                          AND vi.created_by_id = ",
        );
        sql.push(Frag::bind(Value::Uuid(user)));
        sql.push_raw(")))");

        // `Issue.issue_objects` keeps only live projects, so the view's extra
        // `project__archived_at__isnull=True` is already covered above.

        // `:2489-2490`.
        if workspace_search == "false"
            && let Some(project_id) = project_id
        {
            let project_id = Uuid::parse_str(&project_id).map_err(|_| ViewError::Validation)?;
            sql.push_raw(" AND i.project_id = ");
            sql.push(Frag::bind(Value::Uuid(project_id)));
        }

        // `DISTINCT` plus the model's `-created_at` ordering makes Django add
        // the ordering column to the `SELECT` list
        // (`django/db/models/sql/compiler.py:get_order_by` +
        // `Query.set_values`), so the projection carries it too; the row set is
        // the same because `id` is unique.
        sql.push_raw(" ORDER BY i.created_at DESC LIMIT ");
        sql.push(Frag::bind(Value::Int8(limit)));

        let rows = sql
            .into_sql()
            .query(client)
            .await
            .map_err(ViewError::from)?;
        let mut results = Vec::with_capacity(rows.len());
        for row in &rows {
            results.push(SearchRow {
                name: row.try_get(0).map_err(ViewError::from)?,
                id: row.try_get(1).map_err(ViewError::from)?,
                sequence_id: row.try_get(2).map_err(ViewError::from)?,
                project_identifier: row.try_get(3).map_err(ViewError::from)?,
                project_id: row.try_get(4).map_err(ViewError::from)?,
                workspace_slug: row.try_get(5).map_err(ViewError::from)?,
            });
        }
        let response = SearchResponse { rows: &results };
        let prepared = RequestRender::from_ctx(ctx);
        let render_ctx = prepared.render_ctx();
        let body = plane_serializers::render(&response, &render_ctx)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `Response({"issues": []}, status=200)` for a missing `search`
/// (`issue.py:2459-2460`).
fn empty_response(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let empty: Vec<SearchRow> = Vec::new();
    let response = SearchResponse { rows: &empty };
    let prepared = RequestRender::from_ctx(ctx);
    let render_ctx = prepared.render_ctx();
    let body = plane_serializers::render(&response, &render_ctx).map_err(ViewError::from)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

/// `re.findall(r"\b\d+\b", query)`, parsed as Python `int`s.
///
/// A run is `\d+` (Unicode digits in Python's `re`); `\b` on both sides means
/// it is not adjacent to a word character. Values beyond `i64` saturate, which
/// keeps them out of the `int4` comparison below.
fn sequence_terms(query: &str) -> Vec<i64> {
    let chars: Vec<char> = query.chars().collect();
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut terms = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            index += 1;
            continue;
        }
        let boundary_before = start == 0 || !is_word(chars[start - 1]);
        let boundary_after = index == chars.len() || !is_word(chars[index]);
        if boundary_before && boundary_after {
            let mut value: i64 = 0;
            for &digit in &chars[start..index] {
                let digit = digit.to_digit(10).unwrap_or(0) as i64;
                value = value.saturating_mul(10).saturating_add(digit);
            }
            terms.push(value);
        }
    }
    terms
}

/// Python's `int(text)`: surrounding ASCII whitespace, an optional sign and
/// single underscores between digits.
fn python_int(text: &str) -> Result<i64, ViewError> {
    let trimmed =
        text.trim_matches(|ch: char| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}'));
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let invalid = || {
        ViewError::Unknown(format!(
            "ValueError: invalid literal for int() with base 10: '{text}'"
        ))
    };
    if digits.is_empty() {
        return Err(invalid());
    }
    let mut value: i64 = 0;
    let mut previous_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if previous_underscore {
                return Err(invalid());
            }
            previous_underscore = true;
            continue;
        }
        let Some(digit) = ch.to_digit(10) else {
            return Err(invalid());
        };
        value = value.saturating_mul(10).saturating_add(i64::from(digit));
        previous_underscore = false;
    }
    if previous_underscore {
        return Err(invalid());
    }
    Ok(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_int_matches_the_builtin() {
        for (input, expected) in [
            ("10", Some(10)),
            (" 10 ", Some(10)),
            ("+10", Some(10)),
            ("-10", Some(-10)),
            ("1_0", Some(10)),
            ("0", Some(0)),
            ("", None),
            (" ", None),
            ("1__0", None),
            ("_10", None),
            ("10_", None),
            ("x", None),
            ("1.5", None),
        ] {
            match (python_int(input), expected) {
                (Ok(value), Some(expected)) => assert_eq!(value, expected, "{input:?}"),
                (Err(_), None) => {}
                (actual, expected) => panic!("{input:?}: {actual:?}, expected {expected:?}"),
            }
        }
    }

    #[test]
    fn sequence_terms_are_word_bounded_runs() {
        assert_eq!(sequence_terms("cache"), Vec::<i64>::new());
        assert_eq!(sequence_terms("2"), vec![2]);
        assert_eq!(sequence_terms("ALP-2"), vec![2]);
        assert_eq!(sequence_terms("a2"), Vec::<i64>::new());
        assert_eq!(sequence_terms("2a"), Vec::<i64>::new());
        assert_eq!(sequence_terms("2 3"), vec![2, 3]);
        assert_eq!(sequence_terms("x_2"), Vec::<i64>::new());
    }
}
