// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two issue reads and the intake-issue reads of the public board.
//!
//! * `ProjectIssuesPublicEndpoint.get` (`plane/space/views/issue.py:109-244`,
//!   route 248) — the paginated issue list;
//! * `IssueRetrievePublicEndpoint.get` (`issue.py:689-865`, route 238) — one
//!   issue's `.values()` dict;
//! * `IntakeIssuePublicViewSet.list`/`retrieve`
//!   (`plane/space/views/intake.py:57-106,250-270`, routes 234-236 GET) — the
//!   intake's issues through `IssueStateIntakeSerializer`.
//!
//! ## 238: `IssueRetrievePublicEndpoint`
//!
//! The board is `DeployBoard.objects.get(anchor=anchor)` with **no**
//! `entity_name` filter (`issue.py:690`), so a page board answers too; a miss
//! is the family 404. The issue query is `Issue.issue_objects` scoped to the
//! board's `workspace.slug` and `project_id`; a miss is not a 404 — `.first()`
//! returns `None` and the view answers `null` with a 200 (`:863-865`).
//!
//! The projection is a `.values()` call, so the response is an object whose
//! keys follow the `values()` argument order and whose values are the raw
//! columns: `estimate_point` is `estimate_point_id`, `created_by` is the FK
//! column, and the two aggregate columns are JSON arrays rendered from jsonb
//! (canonical jsonb key order inside each element).
//!
//! Two upstream bugs are ported (see [`super::queries::ISSUE_VALUES`]):
//! `reaction_items`' `avatar_url` reads `votes__actor__avatar_asset`
//! (`:805,813`), and the `assignee_ids` filter uses `assignees__member_project`
//! (any project membership) rather than a project-scoped one.
//!
//! ## 248: `ProjectIssuesPublicEndpoint`
//!
//! `issue_filters(request.query_params, "GET")` then
//! `order_issue_queryset(…)` then the *space* grouper
//! (`plane/space/utils/grouper.py`), which differs from the app grouper: the
//! three id arrays are `ArrayAgg` over the through-table joins (not
//! subqueries), and `issue_on_results`' field list is a different, shorter one.
//! `self.paginate` then runs `OffsetPaginator` / `GroupedOffsetPaginator` /
//! `SubGroupedOffsetPaginator` over that queryset.
//!
//! ## 234-236: intake issues
//!
//! `IntakeIssuePublicViewSet.list` (`intake.py:57-106`) filters the board's
//! issues by `issue_intake__intake_id` and renders
//! `IssueStateIntakeSerializer` — the **app** serializer
//! (`intake.py:20-25` imports `plane.app.serializers`, not the space copy, so
//! there is no `bridge_id` field). `retrieve` (`:250-270`) resolves one
//! `IntakeIssue` in the board's workspace/project/intake and renders the same
//! serializer for its issue.
//!
//! Both start with `DeployBoard.objects.get(anchor, entity_name="project")`
//! (a miss is the family 404) and answer
//! `{"error": "Intake is not enabled for this Project Board"}` 400 when the
//! board has no intake (`:59-63`, `:252-256`).

use http::Method;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;
use plane_serde::Dialect;
use tokio_postgres::Row;
use uuid::Uuid;

use super::board;
use super::ok;
use super::queries;
use crate::common::client;

/// Register the two issue reads.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.space.views.issue.IssueRetrievePublicEndpoint")?
        .method(Method::GET, issue_retrieve)?;
    registry
        .api_view("plane.space.views.issue.ProjectIssuesPublicEndpoint")?
        .method(Method::GET, issues)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 238 — IssueRetrievePublicEndpoint
// ---------------------------------------------------------------------------

/// `IssueRetrievePublicEndpoint.get` (`plane/space/views/issue.py:689-865`).
fn issue_retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id: Uuid = ctx
            .kwarg("issue_id")
            .ok_or(ViewError::Key)?
            .parse()
            .map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;
        // `DeployBoard.objects.get(anchor=anchor)` — no `entity_name` filter.
        let deploy_board = board::require_board(client, anchor).await?;
        let slug = super::workspace_slug(client, deploy_board.workspace_id).await?;
        let statement = queries::ISSUE_VALUES.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&issue_id, &deploy_board.project_id, &slug])
            .await?;
        let Some(row) = row else {
            // `.first()` → `None` → `Response(None)`: `JSONRenderer.render`
            // returns `b""` and `Response.rendered_content` then deletes
            // `Content-Type`, so the 200 has neither a body nor that header
            // (`issue.py:863-865`).
            return Ok(ViewResponse::empty(200));
        };
        let body = render_issue_values(&row)?;
        Ok(ok(body))
    })
}

/// One `values()` row of [`queries::ISSUE_VALUES`], in the argument order of
/// the `.values(...)` call (`plane/space/views/issue.py:838-862`).
fn render_issue_values(row: &Row) -> Result<Vec<u8>, ViewError> {
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    out.push(b'{');
    let mut first = true;
    let mut field = |key: &str, value: &[u8], out: &mut Vec<u8>| {
        if !first {
            out.push(b',');
        }
        first = false;
        push_key(out, key);
        out.extend_from_slice(b":");
        out.extend_from_slice(value);
    };

    field("id", &value_bytes(&Json::uuid(read(row, 0)?)), &mut out);
    field(
        "name",
        &value_bytes(&Json::string(read::<String>(row, 1)?)),
        &mut out,
    );
    field(
        "state_id",
        &value_bytes(&Json::opt_uuid(read(row, 2)?)),
        &mut out,
    );
    field(
        "sort_order",
        &value_bytes(&Json::Float(read::<f64>(row, 3)?)),
        &mut out,
    );
    field("description_json", &json_column(row, 4)?, &mut out);
    field(
        "description_html",
        &value_bytes(&Json::string(read::<String>(row, 5)?)),
        &mut out,
    );
    field(
        "description_stripped",
        &value_bytes(&Json::opt_string(read::<Option<String>>(row, 6)?)),
        &mut out,
    );
    // A non-NULL `bytea` is a `memoryview` in Python and `JSONEncoder` refuses
    // it, so Django answers the family 500 (`description_binary` is always
    // NULL in practice).
    let binary: Option<Vec<u8>> = read(row, 7)?;
    if binary.is_some() {
        return Err(ViewError::Unknown(
            "Object of type memoryview is not JSON serializable".to_owned(),
        ));
    }
    field("description_binary", b"null", &mut out);
    field("module_ids", &uuid_array(row, 8)?, &mut out);
    field("label_ids", &uuid_array(row, 9)?, &mut out);
    field("assignee_ids", &uuid_array(row, 10)?, &mut out);
    field(
        "estimate_point",
        &value_bytes(&Json::opt_uuid(read(row, 11)?)),
        &mut out,
    );
    field(
        "priority",
        &value_bytes(&Json::string(read::<String>(row, 12)?)),
        &mut out,
    );
    field(
        "start_date",
        &value_bytes(&Json::opt_date(read(row, 13)?)),
        &mut out,
    );
    field(
        "target_date",
        &value_bytes(&Json::opt_date(read(row, 14)?)),
        &mut out,
    );
    field(
        "sequence_id",
        &value_bytes(&Json::Int(i64::from(read::<i32>(row, 15)?))),
        &mut out,
    );
    field(
        "project_id",
        &value_bytes(&Json::uuid(read(row, 16)?)),
        &mut out,
    );
    field(
        "parent_id",
        &value_bytes(&Json::opt_uuid(read(row, 17)?)),
        &mut out,
    );
    field(
        "cycle_id",
        &value_bytes(&Json::opt_uuid(read(row, 18)?)),
        &mut out,
    );
    field(
        "created_by",
        &value_bytes(&Json::opt_uuid(read(row, 19)?)),
        &mut out,
    );
    field(
        "state__group",
        &value_bytes(&Json::opt_string(read::<Option<String>>(row, 20)?)),
        &mut out,
    );
    field("vote_items", &json_array_column(row, 21)?, &mut out);
    field("reaction_items", &json_array_column(row, 22)?, &mut out);
    out.push(b'}');
    Ok(out)
}

/// One scalar rendered at depth 1.
///
/// `JSONRenderer.render(None)` is `b""`, and `plane_serde`'s DRF dialect
/// reproduces that at the document's top level (`serializer.rs:130-137`). A
/// field value is never a document, so wrapping it in a one-element array —
/// whose `[` and `]` are then dropped — is what renders `null` as `null`.
fn value_bytes(value: &Json) -> Vec<u8> {
    let wrapped = Json::Array(vec![value.clone()]).to_drf();
    wrapped
        .get(1..wrapped.len().saturating_sub(1))
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// A JSON column rendered the way `JSONRenderer` would render the parsed
/// value: `plane_serde`'s DRF dialect.
fn json_column(row: &Row, index: usize) -> Result<Vec<u8>, ViewError> {
    let text: Option<plane_db::JsonText> = read(row, index)?;
    let Some(text) = text else {
        return Ok(b"null".to_vec());
    };
    let mut out = Vec::with_capacity(text.as_str().len() + 16);
    plane_serde::transcode_json(Dialect::Drf, text.as_str(), &mut out)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(out)
}

/// `ARRAY_AGG(DISTINCT <jsonb>)`: `NULL` when the aggregate's `FILTER` matched
/// no row (Django's `None`), otherwise a JSON array in the aggregate's order.
fn json_array_column(row: &Row, index: usize) -> Result<Vec<u8>, ViewError> {
    let items: Option<Vec<plane_db::JsonText>> = read(row, index)?;
    let Some(items) = items else {
        return Ok(b"null".to_vec());
    };
    let mut out = Vec::with_capacity(items.len() * 64);
    out.push(b'[');
    for (position, item) in items.iter().enumerate() {
        if position > 0 {
            out.push(b',');
        }
        plane_serde::transcode_json(Dialect::Drf, item.as_str(), &mut out)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
    }
    out.push(b']');
    Ok(out)
}

/// `ARRAY_AGG(DISTINCT …)`: ascending and never NULL (`COALESCE(…, '{}')`).
fn uuid_array(row: &Row, index: usize) -> Result<Vec<u8>, ViewError> {
    let mut ids: Vec<Uuid> = read::<Vec<Uuid>>(row, index)?;
    ids.sort();
    Ok(Json::Array(ids.into_iter().map(Json::uuid).collect()).to_drf())
}

fn read<T>(row: &Row, index: usize) -> Result<T, ViewError>
where
    T: for<'a> tokio_postgres::types::FromSql<'a>,
{
    row.try_get(index).map_err(ViewError::from)
}

fn push_key(out: &mut Vec<u8>, key: &str) {
    out.push(b'"');
    out.extend_from_slice(key.as_bytes());
    out.push(b'"');
}

// ---------------------------------------------------------------------------
// 248 — ProjectIssuesPublicEndpoint
// ---------------------------------------------------------------------------

/// `ProjectIssuesPublicEndpoint.get` (`plane/space/views/issue.py:109-244`).
fn issues<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    super::issue_list::issues(ctx)
}
