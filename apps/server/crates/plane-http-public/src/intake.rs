// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Intake work items on `/api/v1/` (routes 288/289).
//!
//! Python: `plane/api/views/intake.py:57-523` with
//! `plane/api/serializers/intake.py:13-164`.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `pr/intake-issues/` | `GET` | `IntakeIssueListCreateAPIEndpoint.get` (`:113-126`) |
//! | same | `POST` | `.post` (`:149-230`) |
//! | `pr/intake-issues/{issue_id}/` | `GET` | `IntakeIssueDetailAPIEndpoint.get` (`:287-294`) |
//! | same | `PATCH` | `.patch` (`:318-454`) |
//! | same | `DELETE` | `.delete` (`:469-523`) |
//!
//! `{issue_id}` is the **Issue** id: the link is resolved by `issue_id`.
//!
//! ## The quirks this set owns
//!
//! * `IntakeIssueDetail.patch` gates the intake serializer on `role > 15`, so a
//!   MEMBER (15) cannot edit intake attributes at all (`:408`);
//! * `intake_id=intake.id` is an `AttributeError` → 500 when `intake_view` is
//!   on but no `Intake` row exists (`:340`, and the create's `:212`);
//! * `request.data.pop("issue")` mutates the body, so the activity payload
//!   (`:442`) carries the body **without** `issue`;
//! * the nested `IssueSerializer` has no `description_json` (it is in
//!   `exclude`), so a body `description_json` is dropped; `description` is the
//!   deprecated alias the intake *create* accepts, not the patch;
//! * the create sanitises `description_html` and ignores
//!   `validate_html_content`'s validity flag (`:197-200`).
//!
//! ## `issue_detail`
//!
//! `IntakeIssueSerializer.issue_detail` is `IssueExpandSerializer`, whose
//! `cycle`/`module` fields source `issue_cycle.cycle` / `issue_module.module`.
//! `issue_cycle` is a *reverse manager* (`CycleIssue.issue`'s
//! `related_name`), so `getattr(manager, "cycle")` raises `AttributeError`;
//! DRF's `Field.get_attribute` converts that to `SkipField` for a non-required
//! field (`rest_framework/fields.py:413-421`), so **both keys are absent** from
//! every response. `labels`/`assignees` are `SerializerMethodField`s that read
//! `self.context["expand"]`, and the v1 views never pass a context, so they
//! are the through rows' ids (`issue.py:860-871`).

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use http::Method;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::intake::IntakeIssue;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json as Doc;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use plane_http_app::assets::workspace_write::{body_get, truthy};
use plane_http_app::common::{client, lookup, render::RequestRender};
use plane_http_app::intake::queries as app_queries;
use plane_http_app::intake::write_queries as app_write_queries;
use plane_http_app::projects::queries as project_queries;
use plane_http_app::states::queries as state_queries;
use plane_http_app::write::{self, Serializer};

use crate::intake_queries as queries;
use crate::issues::{self, ISSUE_FIELDS};
use crate::pyvalue::python_str;

/// `ROLE.GUEST.value` (`plane/app/permissions/base.py:16`).
const ROLE_GUEST: i16 = 5;
/// `ROLE.MEMBER.value`.
const ROLE_MEMBER: i16 = 15;
/// `StateGroup.TRIAGE.value`.
const STATE_GROUP_TRIAGE: &str = "triage";
/// `Issue.PRIORITY_CHOICES` keys (`plane/db/models/issue.py:107-113`).
const PRIORITIES: [&str; 5] = ["urgent", "high", "medium", "low", "none"];
/// `validate_html_content`'s 10 MiB cap (`plane/utils/content_validator.py:16`).
const MAX_HTML: usize = 10 * 1024 * 1024;

/// Register both classes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.intake.IntakeIssueListCreateAPIEndpoint")?
        .method(Method::GET, list)?
        .method(Method::POST, create)?;
    registry
        .api_view("plane.api.views.intake.IntakeIssueDetailAPIEndpoint")?
        .method(Method::GET, retrieve)?
        .method(Method::PATCH, patch)?
        .method(Method::DELETE, destroy)?;
    Ok(())
}

/// `Project.objects.get(workspace__slug, pk)` for the three verbs.
struct ProjectRow {
    workspace_id: Uuid,
    intake_view: bool,
}

async fn load_project(
    client: &Client,
    slug: &str,
    project_id: Uuid,
) -> Result<Option<ProjectRow>, ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_FOR_INTAKE.sql)
        .await?;
    Ok(client
        .query_opt(&statement, &[&slug, &project_id])
        .await?
        .map(|row| ProjectRow {
            workspace_id: row.get(1),
            intake_view: row.get(2),
        }))
}

/// `Intake.objects.filter(workspace__slug, project_id).first()`.
async fn intake_for(
    client: &Client,
    slug: &str,
    project_id: Uuid,
) -> Result<Option<Uuid>, ViewError> {
    let statement = client.prepare_cached(app_queries::INTAKE_ONLY.sql).await?;
    Ok(client
        .query_opt(&statement, &[&slug, &project_id])
        .await?
        .map(|row| row.get(0)))
}

/// `is_restricted_guest(user_id, project_id, slug)` (`api/views/issue.py:201-212`).
async fn is_restricted_guest(
    client: &Client,
    user: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            plane_http_app::assets::queries::PROJECT_IS_RESTRICTED_GUEST.sql,
            &[&slug, &project_id, &user],
        )
        .await?
        .map(|row| row.get(0))
        .unwrap_or(false))
}

// ---------------------------------------------------------------------------
// 288 GET
// ---------------------------------------------------------------------------

/// `IntakeIssueListCreateAPIEndpoint.get` (`api/views/intake.py:113-126`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `intake` then `project` (`:67-74`): a missing project is the family
        // 404 even when the intake lookup also returned nothing.
        let intake = intake_for(client, slug, project_id).await?;
        let Some(project) = load_project(client, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let requested = ctx.expand().unwrap_or_default();
        let prepared = RequestRender::from_ctx(ctx);
        let Some(intake_id) = intake else {
            return empty_page(ctx, &requested, &prepared).await;
        };
        if !project.intake_view {
            return empty_page(ctx, &requested, &prepared).await;
        }

        let now = write::now();
        let guest = is_restricted_guest(client, user, project_id, slug).await?;
        let (rows, total, window, next, prev) = if guest {
            crate::projects::fetch_page(
                ctx,
                client,
                queries::INTAKE_ISSUES_PAGE_FOR_CREATOR.sql,
                &[&now, &slug, &project_id, &intake_id, &user],
                queries::INTAKE_ISSUES_COUNT_FOR_CREATOR.sql,
                &[&now, &slug, &project_id, &intake_id, &user],
                1000,
                decode_link,
            )
            .await?
        } else {
            crate::projects::fetch_page(
                ctx,
                client,
                queries::INTAKE_ISSUES_PAGE.sql,
                &[&now, &slug, &project_id, &intake_id],
                queries::INTAKE_ISSUES_COUNT.sql,
                &[&now, &slug, &project_id, &intake_id],
                1000,
                decode_link,
            )
            .await?
        };

        let body = render_page(ctx, client, &rows, &requested, &prepared).await?;
        Ok(crate::projects::page_envelope(
            total,
            &window,
            &next,
            &prev,
            rows.len(),
            body,
        ))
    })
}

/// `IntakeIssue.objects.none()` through `self.paginate` (`:74-75`, `:252-253`).
async fn empty_page(
    ctx: &ViewCtx<'_>,
    requested: &[&str],
    prepared: &RequestRender<'_>,
) -> Result<ViewResponse, ViewError> {
    let _ = (requested, prepared);
    let page = crate::common::page::default_query(ctx)?;
    let window =
        plane_query::paginator::OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let (next, prev) = window.cursors(0);
    Ok(crate::projects::page_envelope(
        0,
        &window,
        &next,
        &prev,
        0,
        b"[]".to_vec(),
    ))
}

// ---------------------------------------------------------------------------
// 288 POST
// ---------------------------------------------------------------------------

/// `IntakeIssueListCreateAPIEndpoint.post` (`api/views/intake.py:149-230`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `if not request.data.get("issue", {}).get("name", False)` (`:155`).
        let issue_value = body_get(data, "issue").unwrap_or_else(|| Json::Dict(PyDict::new()));
        if !matches!(issue_value, Json::Dict(_)) {
            return Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                python_type_name(&issue_value)
            )));
        }
        let issue_data = &issue_value;
        let name_value = match issue_data {
            Json::Dict(object) => object.get("name").cloned().unwrap_or(Json::Bool(false)),
            _ => unreachable!(),
        };
        if !truthy(&name_value) {
            return Ok(plain_error(400, "Name is required"));
        }

        // `intake` then `project` (`:158-160`).
        let intake = intake_for(client, slug, project_id).await?;
        let Some(project) = load_project(client, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if intake.is_none() && !project.intake_view {
            return Ok(plain_error(
                400,
                "Intake is not enabled for this project enable it through the project's api",
            ));
        }

        // `request.data.get("issue", {}).get("priority", "none") not in [...]`
        // (`:170-177`).
        let priority = match issue_data {
            Json::Dict(object) => object
                .get("priority")
                .cloned()
                .unwrap_or_else(|| Json::Str(PyStr::new("none"))),
            _ => unreachable!(),
        };
        let Some(priority) = priority.as_str() else {
            return Ok(plain_error(400, "Invalid priority"));
        };
        if !PRIORITIES.contains(&priority) {
            return Ok(plain_error(400, "Invalid priority"));
        }

        // `State.triage_objects.filter(...).first()`, else a new Triage state
        // (`:180-191`).
        let triage_statement = client
            .prepare_cached(queries::TRIAGE_STATE_FOR_PROJECT.sql)
            .await?;
        let triage_state = match client
            .query_opt(&triage_statement, &[&project_id, &slug])
            .await?
        {
            Some(row) => row.get(0),
            None => {
                let max_row = client
                    .query_one(state_queries::STATE_SEQUENCE_MAX.sql, &[&project_id])
                    .await?;
                let max_sequence: Option<f64> = max_row.get(0);
                let saved =
                    plane_db::hooks::state::before_save("Triage", true, 65_000.0, max_sequence);
                let now = write::now();
                let state = plane_db::models::state::State {
                    created_at: now,
                    updated_at: now,
                    created_by_id: Some(user),
                    updated_by_id: None,
                    deleted_at: None,
                    id: Uuid::new_v4(),
                    project_id,
                    workspace_id: project.workspace_id,
                    name: "Triage".to_owned(),
                    description: String::new(),
                    color: "#4E5355".to_owned(),
                    slug: saved.slug,
                    sequence: saved.sequence,
                    group: STATE_GROUP_TRIAGE.to_owned(),
                    is_triage: false,
                    default: false,
                    external_source: None,
                    external_id: None,
                };
                let statement = client
                    .prepare_cached(state_queries::STATE_INSERT.sql)
                    .await?;
                client
                    .execute(
                        &statement,
                        &[
                            &state.created_at,
                            &state.updated_at,
                            &state.created_by_id,
                            &state.updated_by_id,
                            &state.deleted_at,
                            &state.id,
                            &state.project_id,
                            &state.workspace_id,
                            &state.name,
                            &state.description,
                            &state.color,
                            &state.slug,
                            &state.sequence,
                            &state.group,
                            &state.is_triage,
                            &state.default,
                            &state.external_source,
                            &state.external_id,
                        ],
                    )
                    .await
                    .map_err(|error| match is_integrity(&error) {
                        true => ViewError::Integrity,
                        false => ViewError::from(error),
                    })?;
                state.id
            }
        };

        // The description fields (`:194-200`).
        let object = match issue_data {
            Json::Dict(object) => object,
            _ => unreachable!(),
        };
        let description_json = match object.get("description").filter(|value| truthy(value)) {
            Some(value) => value.clone(),
            None => match object.get("description_json").filter(|value| truthy(value)) {
                Some(value) => value.clone(),
                None => Json::Dict(PyDict::new()),
            },
        };
        let raw_html = object
            .get("description_html")
            .cloned()
            .unwrap_or_else(|| Json::Str(PyStr::new("<p></p>")));
        let safe_html = sanitized_description_html(&raw_html)?;

        // `Issue.objects.create(...)` (`:201-208`): every other column default.
        let issue_name = python_str(&name_value);
        let now = write::now();
        let mut issue = Issue {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id: project.workspace_id,
            parent_id: None,
            state_id: Some(triage_state),
            point: None,
            estimate_point_id: None,
            name: issue_name,
            description_json: plane_db::JsonText::from_json(&description_json)?,
            description_stripped: issues::strip_html(&safe_html),
            description_html: safe_html,
            description_binary: None,
            priority: priority.to_owned(),
            start_date: None,
            target_date: None,
            sequence_id: 1,
            sort_order: 65_535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: None,
        };
        plane_db::hooks::issue::create_issue_row(client, &mut issue, Some(user), now)
            .await
            .map_err(map_issue_hook)?;

        // `IntakeIssue.objects.create(intake_id=intake.id, ...)` (`:211-216`):
        // `intake.id` on a NULL intake is the AttributeError 500.
        let Some(intake_id) = intake else {
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };
        let link_now = write::now();
        let link = IntakeIssue {
            created_at: link_now,
            updated_at: link_now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id: project.workspace_id,
            intake_id,
            issue_id: issue.id,
            status: -2,
            snoozed_till: None,
            duplicate_to_id: None,
            source: Some("IN_APP".to_owned()),
            source_email: None,
            external_source: None,
            external_id: None,
            extra: plane_db::JsonText::new("{}"),
        };
        let statement = client
            .prepare_cached(queries::INTAKE_ISSUE_INSERT.sql)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &link.created_at,
                    &link.updated_at,
                    &link.created_by_id,
                    &link.updated_by_id,
                    &link.deleted_at,
                    &link.id,
                    &link.project_id,
                    &link.workspace_id,
                    &link.intake_id,
                    &link.issue_id,
                    &link.status,
                    &link.snoozed_till,
                    &link.duplicate_to_id,
                    &link.source,
                    &link.source_email,
                    &link.external_source,
                    &link.external_id,
                    &link.extra,
                ],
            )
            .await
            .map_err(|error| match is_integrity(&error) {
                true => ViewError::Integrity,
                false => ViewError::from(error),
            })?;

        // `issue_activity.delay(type="issue.activity.created", requested_data=
        // json.dumps(request.data), …, intake=str(intake_issue.id))`
        // (`:218-227`).
        let requested = plane_http_app::write::request_data_json_text(ctx)?;
        issues::publish_issue_activity(
            ctx,
            plane_jobs::celery::tasks::IssueActivityCall::with_intake(
                issues::activity_base(
                    "issue.activity.created",
                    requested,
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    plane_jobs::celery::args::TaskArg::Null,
                ),
                link.id.to_string(),
            ),
        )
        .await?;

        let body = render_single(ctx, client, &link).await?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `validate_html_content`'s `description_html` half (`:197-200`): the sanitised
/// string, or `"<p></p>"` when the value is falsy / over 10 MiB / the cleaner
/// fails. A truthy non-string is the `AttributeError` the Python's
/// `html_content.encode` raises.
fn sanitized_description_html(value: &Json) -> Result<String, ViewError> {
    let text = match value {
        Json::Null => return Ok("<p></p>".to_owned()),
        Json::Bool(false) => return Ok("<p></p>".to_owned()),
        Json::Int(value) if value.to_decimal_string() == "0" => return Ok("<p></p>".to_owned()),
        Json::Str(value) => value.as_str().unwrap_or_default(),
        other => {
            return Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'encode'",
                python_type_name(other)
            )));
        }
    };
    if text.is_empty() {
        return Ok("<p></p>".to_owned());
    }
    if text.len() > MAX_HTML {
        return Ok("<p></p>".to_owned());
    }
    Ok(plane_http_app::write::html::sanitize(text))
}

// ---------------------------------------------------------------------------
// 289 GET
// ---------------------------------------------------------------------------

/// `IntakeIssueDetailAPIEndpoint.get` (`api/views/intake.py:287-294`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let issue_id =
            lookup::uuid_kwarg("issue_id", ctx.kwarg("issue_id").ok_or(ViewError::Key)?)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let intake = intake_for(client, slug, project_id).await?;
        let Some(project) = load_project(client, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let Some(intake_id) = intake.filter(|_| project.intake_view) else {
            // `IntakeIssue.objects.none().get(...)` → DoesNotExist → 404.
            return Err(ViewError::ObjectDoesNotExist);
        };

        let now = write::now();
        let statement = client
            .prepare_cached(queries::INTAKE_ISSUE_BY_ISSUE.sql)
            .await?;
        let rows = client
            .query(
                &statement,
                &[&now, &slug, &project_id, &intake_id, &issue_id],
            )
            .await?;
        let link = match rows.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            [row] => decode_link(row)?,
            _ => {
                return Err(ViewError::Unknown(
                    "IntakeIssue.MultipleObjectsReturned: the get() matched more than one row"
                        .to_owned(),
                ));
            }
        };
        if is_restricted_guest(client, user, project_id, slug).await?
            && link.created_by_id != Some(user)
        {
            return Err(ViewError::ObjectDoesNotExist);
        }

        let requested = ctx.expand().unwrap_or_default();
        let prepared = RequestRender::from_ctx(ctx);
        let body = render_page(
            ctx,
            client,
            std::slice::from_ref(&link),
            &requested,
            &prepared,
        )
        .await?;
        // `Response(serializer.data)`: one object.
        let object = body[1..body.len() - 1].to_vec();
        Ok(ViewResponse::raw(200, APPLICATION_JSON, object))
    })
}

// ---------------------------------------------------------------------------
// 289 PATCH
// ---------------------------------------------------------------------------

/// `IntakeIssueDetailAPIEndpoint.patch` (`api/views/intake.py:318-454`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let issue_id =
            lookup::uuid_kwarg("issue_id", ctx.kwarg("issue_id").ok_or(ViewError::Key)?)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `issue_data = request.data.pop("issue", False)` (`:359`): a non-mapping
        // body is a 500 before any query.
        let popped = PoppedBody::new(ctx)?;

        let intake = intake_for(client, slug, project_id).await?;
        let Some(project) = load_project(client, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if intake.is_none() && !project.intake_view {
            return Ok(plain_error(
                400,
                "Intake is not enabled for this project enable it through the project's api",
            ));
        }
        // `intake_id=intake.id` (`:340`): a project with `intake_view` on and
        // no `Intake` row is the AttributeError 500.
        let Some(intake_id) = intake else {
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };

        let Some(link) = load_link(client, intake_id, issue_id, project_id, slug).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // `ProjectMember.objects.get(...)` (`:344-349`).
        let member_statement = client
            .prepare_cached(queries::PROJECT_MEMBER_FOR_INTAKE.sql)
            .await?;
        let roles = client
            .query(&member_statement, &[&slug, &project_id, &user])
            .await?;
        let role: i16 = match roles.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            [row] => row.get(0),
            _ => {
                return Err(ViewError::Unknown(
                    "ProjectMember.MultipleObjectsReturned: the get() matched more than one row"
                        .to_owned(),
                ));
            }
        };

        // `if project_member.role <= 5 and str(created_by_id) != str(user.id)`
        // (`:352-356`).
        if role <= ROLE_GUEST && link.created_by_id != Some(user) {
            return Ok(plain_error(400, "You cannot edit intake work items"));
        }

        // The nested issue (`:364-405`), only when `bool(issue_data)`.
        let mut nested = None;
        if truthy(&popped.issue) {
            match prepare_nested(
                ctx,
                client,
                slug,
                project_id,
                issue_id,
                &popped.issue,
                role <= ROLE_GUEST,
            )
            .await?
            {
                Prepared::Ready(value) => nested = Some(value),
                Prepared::Invalid(response) => return Ok(response),
            }
        }

        // The intake serializer only for role > MEMBER (`:408-412`).
        let mut intake_edit = None;
        if role > ROLE_MEMBER {
            match prepare_intake(ctx, client, &link).await? {
                Prepared::Ready(value) => intake_edit = Some(value),
                Prepared::Invalid(response) => return Ok(response),
            }
        }

        // `if issue_serializer: … issue_serializer.save()` (`:415-432`).
        if let Some(nested) = nested {
            save_nested(client, &nested, user).await?;
            issues::publish_issue_activity(
                ctx,
                plane_jobs::celery::tasks::IssueActivityCall::with_intake(
                    issues::activity_base(
                        "issue.activity.updated",
                        plane_jobs::celery::args::TaskArg::json_text(nested.requested_data.clone()),
                        user,
                        &issue_id.to_string(),
                        &project_id.to_string(),
                        plane_jobs::celery::args::TaskArg::json_text(
                            nested.current_instance.clone(),
                        ),
                    ),
                    link.id.to_string(),
                ),
            )
            .await?;
        }

        // `if intake_serializer: … intake_serializer.save()` (`:435-452`).
        let mut saved_link = link.clone();
        if let Some(edit) = &intake_edit {
            save_intake(client, edit, &mut saved_link, user).await?;
            let origin = issues::base_host(ctx)?;
            let requested = plane_http_app::write::request_data_json_text(ctx)?;
            issues::publish_issue_activity(
                ctx,
                plane_jobs::celery::tasks::IssueActivityCall::view_with_intake(
                    issues::activity_base(
                        "intake.activity.created",
                        requested,
                        user,
                        &issue_id.to_string(),
                        &project_id.to_string(),
                        plane_jobs::celery::args::TaskArg::json_text(edit.current_instance.clone()),
                    ),
                    origin,
                    false,
                    link.id.to_string(),
                ),
            )
            .await?;
        }

        let body = render_single(ctx, client, &saved_link).await?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `request.data` after `pop("issue", False)` (`:359`).
struct PoppedBody {
    issue: Json,
}

impl PoppedBody {
    fn new(ctx: &ViewCtx<'_>) -> Result<Self, ViewError> {
        match ctx.data()? {
            plane_drf::parsers::Data::Json(Json::Dict(object)) => Ok(Self {
                issue: object.get("issue").cloned().unwrap_or(Json::Bool(false)),
            }),
            plane_drf::parsers::Data::Json(other) => Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'pop'",
                python_type_name(other)
            ))),
            plane_drf::parsers::Data::Form(query) => {
                // `FormParser`/`MultipartParser` build an immutable `QueryDict`;
                // `.pop` is `AttributeError: This QueryDict instance is
                // immutable` (a 500) unless a file part forced the `.copy()`
                // branch.
                if !query.has_files() {
                    return Err(ViewError::Unknown(
                        "AttributeError: This QueryDict instance is immutable".to_owned(),
                    ));
                }
                let value = match query.get("issue") {
                    Some(value) => Json::List(vec![Json::Str(PyStr::new(value))]),
                    None => Json::Bool(false),
                };
                Ok(Self { issue: value })
            }
        }
    }
}

/// A validated edit, or the 400 that replaces it.
enum Prepared<T> {
    Ready(T),
    Invalid(ViewResponse),
}

/// A validated nested issue.
struct Nested {
    issue: Issue,
    extra: issues::WriteExtra,
    current_instance: String,
    requested_data: String,
}

/// `IssueSerializer(issue, data=issue_data, partial=True, context={...})`
/// (`api/views/intake.py:365-405`).
async fn prepare_nested(
    ctx: &ViewCtx<'_>,
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    issue_data: &Json,
    is_project_guest: bool,
) -> Result<Prepared<Nested>, ViewError> {
    // `Issue.objects.annotate(...).get(pk=…, workspace__slug, project_id)`
    // (`:365-386`): the default manager, so a soft-deleted issue is a 404.
    let statement = client
        .prepare_cached(queries::ISSUE_FOR_INTAKE_DELETE.sql)
        .await?;
    let Some(row) = client
        .query_opt(&statement, &[&slug, &project_id, &issue_id])
        .await?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let mut issue = Issue::from_row(&row, 0)?;

    // `if project_member.role <= 5: issue_data = {"name", "description_html",
    // "description_json"}` (`:389-395`).
    let reduced;
    let issue_data = if is_project_guest {
        let Json::Dict(object) = issue_data else {
            return Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                python_type_name(issue_data)
            )));
        };
        let mut map = PyDict::with_capacity(3);
        map.insert(
            PyStr::new("name"),
            object
                .get("name")
                .cloned()
                .unwrap_or_else(|| Json::Str(PyStr::new(&issue.name))),
        );
        map.insert(
            PyStr::new("description_html"),
            object
                .get("description_html")
                .cloned()
                .unwrap_or_else(|| Json::Str(PyStr::new(&issue.description_html))),
        );
        map.insert(
            PyStr::new("description_json"),
            object.get("description_json").cloned().unwrap_or_else(|| {
                plane_serde::pyjson::loads(issue.description_json.as_str().as_bytes(), "utf-8")
                    .unwrap_or(Json::Null)
            }),
        );
        reduced = Json::Dict(map);
        &reduced
    } else {
        issue_data
    };

    // `IssueSerializer(issue).data` pre-image (`:412`).
    let bundle = issues::bundle_after_write(client, issue_id).await?;
    let current_instance = issues::current_instance(ctx, &bundle)?;

    // `IssueSerializer(issue, data=…, partial=True)` (`:397-402`).
    let mut serializer = Serializer::from_value(ctx, issue_data, true);
    issues::validate_fields(&mut serializer, client, true).await?;
    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(mut errors) => {
            errors.sort_by_field_order(ISSUE_FIELDS);
            return Ok(Prepared::Invalid(errors.into_response()));
        }
    };
    let extra =
        match issues::validate_extra(client, &validated, project_id, issue.workspace_id).await {
            Ok(extra) => extra,
            Err(errors) => return Ok(Prepared::Invalid(errors.into_response())),
        };

    // `writable_data(issue_serializer, issue_data)` (`:418`) — the read-only
    // keys are dropped, a non-mapping value is the `.items()` AttributeError.
    let requested_data = nested_writable_data(issue_data, true)?;

    issues::apply_patch(&mut issue, &validated, &extra);
    Ok(Prepared::Ready(Nested {
        issue,
        extra,
        current_instance,
        requested_data,
    }))
}

/// `IssueSerializer.update`'s m2m replacement plus `Issue.save()`
/// (`api/serializers/issue.py:267-321`, `plane/db/models/issue.py:180-255`).
async fn save_nested(client: &Client, nested: &Nested, user: Uuid) -> Result<(), ViewError> {
    let mut issue = nested.issue.clone();
    let m2m_now = write::now();
    if let Some(assignees) = &nested.extra.assignees {
        plane_db::hooks::issue_m2m::replace_assignees(client, &issue, assignees, m2m_now)
            .await
            .map_err(ViewError::from)?;
    }
    if let Some(labels) = &nested.extra.labels {
        plane_db::hooks::issue_m2m::replace_labels(client, &issue, labels, m2m_now)
            .await
            .map_err(ViewError::from)?;
    }
    let old_state = issue.state_id;
    if issue.state_id.is_none() {
        issue.state_id = plane_db::hooks::issue::ensure_default_state(client, issue.project_id)
            .await
            .map_err(ViewError::from)?;
    }
    let state_changed = issue.state_id != old_state;
    issue.description_stripped = issues::strip_html(&issue.description_html);
    plane_db::hooks::issue::update_issue_row(
        client,
        &mut issue,
        state_changed,
        Some(user),
        write::now(),
    )
    .await
    .map_err(ViewError::from)?;
    Ok(())
}

/// A validated intake edit.
struct IntakeEdit {
    status: Option<i64>,
    snoozed_till: Option<DateTime<Utc>>,
    duplicate_to: Option<Option<Uuid>>,
    source: Option<Option<String>>,
    source_email: Option<Option<String>>,
    current_instance: String,
    /// `issue.state.group`, for the accept transition.
    state_group: Option<String>,
}

/// `IntakeIssueUpdateSerializer(intake_issue, data=request.data, partial=True)`
/// (`plane/api/serializers/intake.py:84-164`).
async fn prepare_intake(
    ctx: &ViewCtx<'_>,
    client: &Client,
    link: &IntakeIssue,
) -> Result<Prepared<IntakeEdit>, ViewError> {
    let current_instance = single_pre_image(ctx, client, link).await?;
    let mut serializer = Serializer::new(ctx, true)?;
    plane_http_app::write::generated::intake_issue_update_serializer::validate(
        &mut serializer,
        client,
    )
    .await?;
    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(errors) => return Ok(Prepared::Invalid(errors.into_response())),
    };

    // `if attrs.get("snoozed_till"): attrs["snoozed_till"] = snooze_day_start(
    // self.initial_data.get("snoozed_till"), self.instance.project_id) or …`
    // (`intake.py:120-124`).
    let mut snoozed_till = validated.datetime("snoozed_till");
    if snoozed_till.is_some() {
        let initial = initial_value(ctx, "snoozed_till")?;
        if let Some(local) = snooze_day_start(client, initial.as_ref(), link.project_id).await? {
            snoozed_till = Some(local);
        }
    }

    // `issue.state.group` for both the accept guard and the transition.
    let state_group = issue_state_group(client, link.issue_id).await?;
    if validated.integer("status") == Some(1)
        && state_group.as_deref() == Some(STATE_GROUP_TRIAGE)
        && default_state(client, link.workspace_id, link.project_id)
            .await?
            .is_none()
    {
        return Ok(Prepared::Invalid(write::field_string_error(
            "status",
            "Cannot accept intake issue: No default state found for the project",
        )));
    }

    Ok(Prepared::Ready(IntakeEdit {
        status: validated.integer("status"),
        snoozed_till,
        duplicate_to: validated
            .contains("duplicate_to")
            .then(|| validated.uuid("duplicate_to")),
        source: validated
            .contains("source")
            .then(|| validated.text("source").map(str::to_owned)),
        source_email: validated
            .contains("source_email")
            .then(|| validated.text("source_email").map(str::to_owned)),
        current_instance,
        state_group,
    }))
}

/// `IntakeIssueUpdateSerializer.update` (`plane/api/serializers/intake.py:145-164`).
async fn save_intake(
    client: &Client,
    edit: &IntakeEdit,
    link: &mut IntakeIssue,
    user: Uuid,
) -> Result<(), ViewError> {
    // H16: `if self.snoozed_till and (adding or has_changed("snoozed_till"))`.
    let requested = match &edit.snoozed_till {
        Some(value) => Some(*value),
        None => link.snoozed_till,
    };
    let snoozed_till = match requested {
        Some(value) if Some(value) != link.snoozed_till => {
            let timezone = project_timezone(client, link.project_id).await?;
            plane_db::hooks::intake_issue::before_save(Some(value), false, true, timezone)
        }
        other => other,
    };
    link.snoozed_till = snoozed_till;
    if let Some(status) = edit.status {
        link.status = status as i32;
    }
    if let Some(duplicate_to) = edit.duplicate_to {
        link.duplicate_to_id = duplicate_to;
    }
    if let Some(source) = &edit.source {
        link.source = source.clone();
    }
    if let Some(source_email) = &edit.source_email {
        link.source_email = source_email.clone();
    }
    link.updated_at = write::now();
    link.updated_by_id = Some(user);
    let statement = client
        .prepare_cached(queries::INTAKE_ISSUE_UPDATE.sql)
        .await?;
    client
        .execute(
            &statement,
            &[
                &link.created_at,
                &link.updated_at,
                &link.created_by_id,
                &link.updated_by_id,
                &link.deleted_at,
                &link.id,
                &link.project_id,
                &link.workspace_id,
                &link.intake_id,
                &link.issue_id,
                &link.status,
                &link.snoozed_till,
                &link.duplicate_to_id,
                &link.source,
                &link.source_email,
                &link.external_source,
                &link.external_id,
                &link.extra,
                &link.id,
            ],
        )
        .await
        .map_err(|error| match is_integrity(&error) {
            true => ViewError::Integrity,
            false => ViewError::from(error),
        })?;

    // `if validated_data.get("status") == 1:` — the accept transition
    // (`intake.py:153-163`).
    if edit.status == Some(1)
        && edit.state_group.as_deref() == Some(STATE_GROUP_TRIAGE)
        && let Some(state_id) = default_state(client, link.workspace_id, link.project_id).await?
    {
        let Some(mut issue) = load_issue_any(client, link.issue_id).await? else {
            return Err(ViewError::Unknown(
                "the intake link's issue does not exist".to_owned(),
            ));
        };
        issue.state_id = Some(state_id);
        issue.description_stripped = issues::strip_html(&issue.description_html);
        plane_db::hooks::issue::update_issue_row(
            client,
            &mut issue,
            true,
            Some(user),
            write::now(),
        )
        .await
        .map_err(ViewError::from)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 289 DELETE
// ---------------------------------------------------------------------------

/// `IntakeIssueDetailAPIEndpoint.delete` (`api/views/intake.py:469-523`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let issue_id =
            lookup::uuid_kwarg("issue_id", ctx.kwarg("issue_id").ok_or(ViewError::Key)?)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let intake = intake_for(client, slug, project_id).await?;
        let Some(project) = load_project(client, slug, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if intake.is_none() && !project.intake_view {
            return Ok(plain_error(
                400,
                "Intake is not enabled for this project enable it through the project's api",
            ));
        }
        let Some(intake_id) = intake else {
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };
        let Some(link) = load_link(client, intake_id, issue_id, project_id, slug).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // A restricted guest who did not submit it is a 404 (`:493-495`).
        if link.created_by_id != Some(user)
            && is_restricted_guest(client, user, project_id, slug).await?
        {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `Issue.objects.filter(workspace__slug, project_id, pk).first()`
        // (`:498`).
        let statement = client
            .prepare_cached(queries::ISSUE_FOR_INTAKE_DELETE.sql)
            .await?;
        let issue = client
            .query_opt(&statement, &[&slug, &project_id, &issue_id])
            .await?
            .map(|row| Issue::from_row(&row, 0))
            .transpose()?;

        // `(issue is not None and issue.created_by_id == user) or
        // ProjectMember(role=20) or WorkspaceMember(role=20)` (`:499-511`).
        let project_admin = client
            .query_opt(
                project_queries::PROJECT_ADMIN_EXISTS.sql,
                &[&user, &slug, &project_id],
            )
            .await?
            .is_some();
        let workspace_admin = client
            .query_opt(project_queries::WORKSPACE_ADMIN_EXISTS.sql, &[&user, &slug])
            .await?
            .is_some();
        let creator = issue
            .as_ref()
            .is_some_and(|issue| issue.created_by_id == Some(user));
        if !(creator || project_admin || workspace_admin) {
            return Ok(plain_error(
                403,
                "Only admin or creator can delete the work item",
            ));
        }

        // `if intake_issue.status in [-2, -1, 0, 2]: issue.delete()` (`:517-520`).
        if matches!(link.status, -2 | -1 | 0 | 2) {
            let Some(mut issue) = issue else {
                return Err(ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'delete'".to_owned(),
                ));
            };
            let old_state = issue.state_id;
            if issue.state_id.is_none() {
                issue.state_id =
                    plane_db::hooks::issue::ensure_default_state(client, issue.project_id)
                        .await
                        .map_err(ViewError::from)?;
            }
            let state_changed = issue.state_id != old_state;
            let now = write::now();
            issue.deleted_at = Some(now);
            issue.description_stripped = issues::strip_html(&issue.description_html);
            plane_db::hooks::issue::update_issue_row(
                client,
                &mut issue,
                state_changed,
                Some(user),
                now,
            )
            .await
            .map_err(ViewError::from)?;
            write::publish_soft_delete_cascade(ctx, "db", "issue", issue_id).await?;
        }

        // `intake_issue.delete()` (H03).
        write::soft_delete(
            client,
            "intake_issues",
            "id",
            link.id,
            Some(user),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "intakeissue", link.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Rendering (`IntakeIssueSerializer`, `plane/api/serializers/intake.py:58-81`)
// ---------------------------------------------------------------------------

/// `IntakeIssueSerializer.FIELDS` (`_readable_fields` order).
const INTAKE_FIELDS: &[&str] = &[
    "id",
    "issue_detail",
    "inbox",
    "created_at",
    "updated_at",
    "deleted_at",
    "status",
    "snoozed_till",
    "source",
    "source_email",
    "external_source",
    "external_id",
    "extra",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "intake",
    "issue",
    "duplicate_to",
];

/// `IssueExpandSerializer.FIELDS` minus the two always-skipped relations.
const ISSUE_DETAIL_FIELDS: &[&str] = &[
    "id",
    "labels",
    "assignees",
    "state",
    "description",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "estimate_point",
    "type",
];

/// Everything the nested `issue_detail` needs.
struct IssueDetailRow {
    issue: Issue,
    state: Option<StateLiteRow>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
}

struct StateLiteRow {
    id: Uuid,
    name: String,
    color: String,
    group: String,
}

/// One `intake_issues` row for rendering.
fn decode_link(row: &tokio_postgres::Row) -> Result<IntakeIssue, ViewError> {
    IntakeIssue::from_row(row, 0).map_err(ViewError::from)
}

/// Render a whole page: one JSON array of `IntakeIssueSerializer` objects.
async fn render_page(
    ctx: &ViewCtx<'_>,
    client: &Client,
    links: &[IntakeIssue],
    requested: &[&str],
    _prepared: &RequestRender<'_>,
) -> Result<Vec<u8>, ViewError> {
    let fields = ctx.fields().unwrap_or_default();
    let details = load_details(client, links).await?;
    let expansions = if requested.is_empty() {
        None
    } else {
        Some(load_expansion_sources(client, links, requested).await?)
    };

    let mut out = Vec::with_capacity(links.len() * 1024 + 2);
    out.push(b'[');
    for (index, link) in links.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        let detail = details.get(&link.issue_id);
        let document = link_document(ctx, link, detail, &fields, requested, expansions.as_ref())?;
        out.extend_from_slice(&document.to_drf());
    }
    out.push(b']');
    Ok(out)
}

/// Render one link as an object (the create/patch responses).
async fn render_single(
    ctx: &ViewCtx<'_>,
    client: &Client,
    link: &IntakeIssue,
) -> Result<Vec<u8>, ViewError> {
    let details = load_details(client, std::slice::from_ref(link)).await?;
    let detail = details.get(&link.issue_id);
    let document = link_document(ctx, link, detail, &[], &[], None)?;
    Ok(document.to_drf())
}

/// `json.dumps(IntakeIssueSerializer(link).data, cls=DjangoJSONEncoder)` — the
/// `intake.activity.created` pre-image (`api/views/intake.py:436`).
async fn single_pre_image(
    ctx: &ViewCtx<'_>,
    client: &Client,
    link: &IntakeIssue,
) -> Result<String, ViewError> {
    let details = load_details(client, std::slice::from_ref(link)).await?;
    let detail = details.get(&link.issue_id);
    let document = link_document(ctx, link, detail, &[], &[], None)?;
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &document)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// Everything `expand=` may replace, loaded once per page.
struct ExpansionSources {
    workspaces: HashMap<Uuid, plane_http_app::projects::lite::LiteWorkspace>,
    users: HashMap<Uuid, plane_http_app::projects::lite::LiteUser>,
    projects: HashMap<Uuid, crate::labels::ProjectLite>,
}

async fn load_expansion_sources(
    client: &Client,
    links: &[IntakeIssue],
    requested: &[&str],
) -> Result<ExpansionSources, ViewError> {
    let mut workspace_ids: HashSet<Uuid> = HashSet::new();
    let mut project_ids: HashSet<Uuid> = HashSet::new();
    let mut user_ids: HashSet<Uuid> = HashSet::new();
    for link in links {
        if requested.contains(&"workspace") {
            workspace_ids.insert(link.workspace_id);
        }
        if requested.contains(&"project") {
            project_ids.insert(link.project_id);
        }
        if requested.contains(&"created_by") {
            user_ids.extend(link.created_by_id);
        }
        if requested.contains(&"updated_by") {
            user_ids.extend(link.updated_by_id);
        }
    }
    let mut workspaces =
        plane_http_app::projects::lite::load_workspaces(client, &workspace_ids).await?;
    let users = plane_http_app::projects::lite::load_users(client, &user_ids).await?;
    let projects = crate::labels::load_projects(client, &project_ids).await?;
    let mut asset_ids = plane_http_app::projects::lite::asset_ids(
        &HashMap::<Uuid, plane_http_app::projects::lite::LiteProject>::new(),
        &workspaces,
        &users,
    );
    asset_ids.extend(
        projects
            .values()
            .filter_map(|project| project.cover_image_asset_id),
    );
    let urls = plane_http_app::projects::lite::load_asset_urls(client, &asset_ids).await?;
    plane_http_app::projects::lite::apply_workspaces(&mut workspaces, &urls)?;
    Ok(ExpansionSources {
        workspaces,
        users,
        projects,
    })
}

/// The nested issue rows for a page: issues, states, label ids, assignee ids.
async fn load_details(
    client: &Client,
    links: &[IntakeIssue],
) -> Result<HashMap<Uuid, IssueDetailRow>, ViewError> {
    let mut issue_ids: Vec<Uuid> = links.iter().map(|link| link.issue_id).collect();
    issue_ids.sort_unstable();
    issue_ids.dedup();
    if issue_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let statement = client
        .prepare_cached(queries::ISSUE_BY_ID_ANY_STATE.sql)
        .await?;
    let mut issues: BTreeMap<Uuid, Issue> = BTreeMap::new();
    for id in &issue_ids {
        let Some(row) = client.query_opt(&statement, &[id]).await? else {
            // `intake_issue.issue` on a dangling FK is `Issue.DoesNotExist`.
            return Err(ViewError::ObjectDoesNotExist);
        };
        let issue = Issue::from_row(&row, 0)?;
        issues.insert(*id, issue);
    }

    let state_ids: Vec<Uuid> = issues.values().filter_map(|issue| issue.state_id).collect();
    let mut states: HashMap<Uuid, StateLiteRow> = HashMap::new();
    if !state_ids.is_empty() {
        let statement = client
            .prepare_cached(queries::STATES_LITE_BY_IDS.sql)
            .await?;
        for row in client.query(&statement, &[&state_ids]).await? {
            let id: Uuid = row.get(0);
            states.insert(
                id,
                StateLiteRow {
                    id,
                    name: row.get(1),
                    color: row.get(2),
                    group: row.get(3),
                },
            );
        }
    }

    let statement = client.prepare_cached(queries::ISSUE_LABEL_IDS.sql).await?;
    let mut labels: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in client.query(&statement, &[&issue_ids]).await? {
        labels
            .entry(row.get::<_, Uuid>(0))
            .or_default()
            .push(row.get(1));
    }
    let statement = client
        .prepare_cached(queries::ISSUE_ASSIGNEE_IDS.sql)
        .await?;
    let mut assignees: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in client.query(&statement, &[&issue_ids]).await? {
        assignees
            .entry(row.get::<_, Uuid>(0))
            .or_default()
            .push(row.get(1));
    }

    let mut out = HashMap::with_capacity(issues.len());
    for (id, issue) in issues {
        let state = issue
            .state_id
            .and_then(|state_id| states.get(&state_id))
            .map(|row| StateLiteRow {
                id: row.id,
                name: row.name.clone(),
                color: row.color.clone(),
                group: row.group.clone(),
            });
        out.insert(
            id,
            IssueDetailRow {
                label_ids: labels.remove(&id).unwrap_or_default(),
                assignee_ids: assignees.remove(&id).unwrap_or_default(),
                state,
                issue,
            },
        );
    }
    Ok(out)
}

/// The `IntakeIssueSerializer` document for one link.
fn link_document(
    ctx: &ViewCtx<'_>,
    link: &IntakeIssue,
    detail: Option<&IssueDetailRow>,
    fields: &[&str],
    requested: &[&str],
    expansions: Option<&ExpansionSources>,
) -> Result<Doc, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let zoned =
        |value: DateTime<Utc>| Doc::DateTimeZoned(value.with_timezone(&timezone).fixed_offset());
    let zoned_opt = |value: Option<DateTime<Utc>>| value.map_or(Doc::Null, zoned);
    let mut object: Vec<(String, Doc)> = Vec::with_capacity(INTAKE_FIELDS.len());
    let emits = |name: &str| fields.is_empty() || fields.contains(&name);
    let mut push = |name: &str, value: Doc| {
        if emits(name) {
            object.push((name.to_owned(), value));
        }
    };

    push("id", Doc::uuid(link.id));
    push(
        "issue_detail",
        match detail {
            Some(detail) => issue_detail_document(detail, &timezone),
            None => Doc::Null,
        },
    );
    push("inbox", Doc::uuid(link.intake_id));
    push("created_at", zoned(link.created_at));
    push("updated_at", zoned(link.updated_at));
    push("deleted_at", zoned_opt(link.deleted_at));
    push("status", Doc::Int(i64::from(link.status)));
    push("snoozed_till", zoned_opt(link.snoozed_till));
    push(
        "source",
        link.source.as_deref().map_or(Doc::Null, Doc::string),
    );
    push(
        "source_email",
        link.source_email.as_deref().map_or(Doc::Null, Doc::string),
    );
    push(
        "external_source",
        link.external_source
            .as_deref()
            .map_or(Doc::Null, Doc::string),
    );
    push(
        "external_id",
        link.external_id.as_deref().map_or(Doc::Null, Doc::string),
    );
    push("extra", json_doc(&link.extra));
    push("created_by", Doc::opt_uuid(link.created_by_id));
    push("updated_by", Doc::opt_uuid(link.updated_by_id));
    push("project", Doc::uuid(link.project_id));
    push("workspace", Doc::uuid(link.workspace_id));
    push("intake", Doc::uuid(link.intake_id));
    push("issue", Doc::uuid(link.issue_id));
    push("duplicate_to", Doc::opt_uuid(link.duplicate_to_id));

    // `expand=` replacements (`plane/api/serializers/base.py:79-141`).
    if !requested.is_empty() {
        for &name in requested {
            if !emits(name) {
                continue;
            }
            let Some(index) = object.iter().position(|(key, _)| key == name) else {
                continue;
            };
            let replacement = match (expansions, name) {
                (Some(sources), "workspace") => {
                    sources.workspaces.get(&link.workspace_id).map(|row| {
                        Doc::Object(vec![
                            ("name".to_owned(), Doc::string(row.name.as_str())),
                            ("slug".to_owned(), Doc::string(row.slug.as_str())),
                            ("id".to_owned(), Doc::uuid(row.id)),
                        ])
                    })
                }
                (Some(sources), "project") => {
                    sources.projects.get(&link.project_id).map(project_document)
                }
                (Some(sources), "created_by") => link
                    .created_by_id
                    .and_then(|id| sources.users.get(&id))
                    .map(user_document),
                (Some(sources), "updated_by") => link
                    .updated_by_id
                    .and_then(|id| sources.users.get(&id))
                    .map(user_document),
                // Every other name is `getattr(instance, "<name>_id", None)`:
                // `issue`/`duplicate_to`/`inbox` have such an attribute, the
                // rest do not.
                (_, "issue") => Some(Doc::uuid(link.issue_id)),
                (_, "duplicate_to") => Some(Doc::opt_uuid(link.duplicate_to_id)),
                (_, "intake") => Some(Doc::uuid(link.intake_id)),
                _ => Some(Doc::Null),
            };
            if let Some(value) = replacement {
                object[index].1 = value;
            }
        }
    }

    let _ = ISSUE_DETAIL_FIELDS;
    Ok(Doc::Object(object))
}

/// `IssueExpandSerializer`'s document, without `cycle`/`module` (module docs).
fn issue_detail_document(detail: &IssueDetailRow, timezone: &chrono_tz::Tz) -> Doc {
    let zoned =
        |value: DateTime<Utc>| Doc::DateTimeZoned(value.with_timezone(timezone).fixed_offset());
    let zoned_opt = |value: Option<DateTime<Utc>>| value.map_or(Doc::Null, zoned);
    let issue = &detail.issue;
    let ids = |values: &[Uuid]| Doc::Array(values.iter().copied().map(Doc::uuid).collect());
    Doc::Object(vec![
        ("id".to_owned(), Doc::uuid(issue.id)),
        ("labels".to_owned(), ids(&detail.label_ids)),
        ("assignees".to_owned(), ids(&detail.assignee_ids)),
        (
            "state".to_owned(),
            detail.state.as_ref().map_or(Doc::Null, |state| {
                Doc::Object(vec![
                    ("id".to_owned(), Doc::uuid(state.id)),
                    ("name".to_owned(), Doc::string(state.name.as_str())),
                    ("color".to_owned(), Doc::string(state.color.as_str())),
                    ("group".to_owned(), Doc::string(state.group.as_str())),
                ])
            }),
        ),
        ("description".to_owned(), json_doc(&issue.description_json)),
        ("created_at".to_owned(), zoned(issue.created_at)),
        ("updated_at".to_owned(), zoned(issue.updated_at)),
        ("deleted_at".to_owned(), zoned_opt(issue.deleted_at)),
        (
            "point".to_owned(),
            issue
                .point
                .map_or(Doc::Null, |point| Doc::Int(i64::from(point))),
        ),
        ("name".to_owned(), Doc::string(issue.name.as_str())),
        (
            "description_json".to_owned(),
            json_doc(&issue.description_json),
        ),
        (
            "description_html".to_owned(),
            Doc::string(issue.description_html.as_str()),
        ),
        (
            "description_stripped".to_owned(),
            issue
                .description_stripped
                .as_deref()
                .map_or(Doc::Null, Doc::string),
        ),
        ("description_binary".to_owned(), Doc::Null),
        ("priority".to_owned(), Doc::string(issue.priority.as_str())),
        ("start_date".to_owned(), Doc::opt_date(issue.start_date)),
        ("target_date".to_owned(), Doc::opt_date(issue.target_date)),
        (
            "sequence_id".to_owned(),
            Doc::Int(i64::from(issue.sequence_id)),
        ),
        ("sort_order".to_owned(), Doc::Float(issue.sort_order)),
        ("completed_at".to_owned(), zoned_opt(issue.completed_at)),
        ("archived_at".to_owned(), Doc::opt_date(issue.archived_at)),
        ("is_draft".to_owned(), Doc::Bool(issue.is_draft)),
        (
            "external_source".to_owned(),
            issue
                .external_source
                .as_deref()
                .map_or(Doc::Null, Doc::string),
        ),
        (
            "external_id".to_owned(),
            issue.external_id.as_deref().map_or(Doc::Null, Doc::string),
        ),
        ("created_by".to_owned(), Doc::opt_uuid(issue.created_by_id)),
        ("updated_by".to_owned(), Doc::opt_uuid(issue.updated_by_id)),
        ("project".to_owned(), Doc::uuid(issue.project_id)),
        ("workspace".to_owned(), Doc::uuid(issue.workspace_id)),
        ("parent".to_owned(), Doc::opt_uuid(issue.parent_id)),
        (
            "estimate_point".to_owned(),
            Doc::opt_uuid(issue.estimate_point_id),
        ),
        ("type".to_owned(), Doc::opt_uuid(issue.type_id)),
    ])
}

fn project_document(project: &crate::labels::ProjectLite) -> Doc {
    Doc::Object(vec![
        ("id".to_owned(), Doc::uuid(project.id)),
        (
            "identifier".to_owned(),
            Doc::string(project.identifier.as_str()),
        ),
        ("name".to_owned(), Doc::string(project.name.as_str())),
        (
            "cover_image".to_owned(),
            project
                .cover_image
                .as_deref()
                .map_or(Doc::Null, Doc::string),
        ),
        (
            "icon_prop".to_owned(),
            project.icon_prop.as_ref().map_or(Doc::Null, json_doc),
        ),
        (
            "emoji".to_owned(),
            project.emoji.as_deref().map_or(Doc::Null, Doc::string),
        ),
        (
            "description".to_owned(),
            Doc::string(project.description.as_str()),
        ),
        (
            "cover_image_url".to_owned(),
            project
                .cover_image_url
                .as_deref()
                .map_or(Doc::Null, Doc::string),
        ),
        ("archived_at".to_owned(), Doc::Null),
    ])
}

fn user_document(user: &plane_http_app::projects::lite::LiteUser) -> Doc {
    Doc::Object(vec![
        ("id".to_owned(), Doc::uuid(user.id)),
        (
            "first_name".to_owned(),
            Doc::string(user.first_name.as_str()),
        ),
        ("last_name".to_owned(), Doc::string(user.last_name.as_str())),
        (
            "email".to_owned(),
            user.email.as_deref().map_or(Doc::Null, Doc::string),
        ),
        ("avatar".to_owned(), Doc::string(user.avatar.as_str())),
        (
            "avatar_url".to_owned(),
            user.avatar_url.as_deref().map_or(Doc::Null, Doc::string),
        ),
        (
            "display_name".to_owned(),
            Doc::string(user.display_name.as_str()),
        ),
    ])
}

/// A `jsonb` column as a document.
fn json_doc(value: &plane_db::JsonText) -> Doc {
    match plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8") {
        Ok(value) => pyjson_to_doc(&value),
        Err(_) => Doc::Null,
    }
}

/// `plane_serde`'s value tree → the order-preserving document.
fn pyjson_to_doc(value: &Json) -> Doc {
    match value {
        Json::Null => Doc::Null,
        Json::Bool(value) => Doc::Bool(*value),
        Json::Int(value) => value
            .as_i64()
            .map_or_else(|| Doc::string(value.to_decimal_string()), Doc::Int),
        Json::Float(value) => Doc::Float(*value),
        Json::Str(value) => Doc::string(value.as_str().unwrap_or_default()),
        Json::List(items) => Doc::Array(items.iter().map(pyjson_to_doc).collect()),
        Json::Dict(map) => Doc::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.as_str().unwrap_or_default().to_owned(),
                        pyjson_to_doc(value),
                    )
                })
                .collect(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn load_link(
    client: &Client,
    intake_id: Uuid,
    issue_id: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<Option<IntakeIssue>, ViewError> {
    let statement = client
        .prepare_cached(app_write_queries::INTAKE_ISSUE_FOR_ISSUE.sql)
        .await?;
    let rows = client
        .query(&statement, &[&intake_id, &issue_id, &project_id, &slug])
        .await?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => decode_link(row).map(Some),
        _ => Err(ViewError::Unknown(
            "IntakeIssue.MultipleObjectsReturned: the get() matched more than one row".to_owned(),
        )),
    }
}

async fn load_issue_any(client: &Client, id: Uuid) -> Result<Option<Issue>, ViewError> {
    let statement = client
        .prepare_cached(queries::ISSUE_BY_ID_ANY_STATE.sql)
        .await?;
    client
        .query_opt(&statement, &[&id])
        .await?
        .map(|row| Issue::from_row(&row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `issue.state.group` for the accept guard: a forward FK, base manager.
async fn issue_state_group(client: &Client, issue_id: Uuid) -> Result<Option<String>, ViewError> {
    let Some(issue) = load_issue_any(client, issue_id).await? else {
        return Ok(None);
    };
    let Some(state_id) = issue.state_id else {
        return Ok(None);
    };
    let statement = client
        .prepare_cached(queries::STATES_LITE_BY_IDS.sql)
        .await?;
    let ids = vec![state_id];
    Ok(client
        .query_opt(&statement, &[&ids])
        .await?
        .map(|row| row.get(3)))
}

/// `State.objects.filter(workspace=…, project=…, default=True).first()`
/// (`plane/api/serializers/intake.py:64-66`).
async fn default_state(
    client: &Client,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<Option<Uuid>, ViewError> {
    Ok(client
        .query_opt(
            plane_http_app::intake::write_intake_issue_patch_queries::INTAKE_ISSUE_PATCH_DEFAULT_STATE
                .sql,
            &[&workspace_id, &project_id],
        )
        .await?
        .map(|row| row.get(0)))
}

/// `Project.objects.values_list("timezone", flat=True).get(id=…)`.
async fn project_timezone(client: &Client, project_id: Uuid) -> Result<chrono_tz::Tz, ViewError> {
    plane_http_app::cycles::base::project_timezone(client, project_id).await
}

/// `snooze_day_start(value, project_id)`
/// (`plane/utils/timezone_converter.py:103-112`): only a bare `YYYY-MM-DD`
/// converts; every other value is `None`.
async fn snooze_day_start(
    client: &Client,
    value: Option<&Json>,
    project_id: Uuid,
) -> Result<Option<DateTime<Utc>>, ViewError> {
    let Some(Json::Str(text)) = value else {
        return Ok(None);
    };
    let Some(text) = text.as_str() else {
        return Ok(None);
    };
    let Some(day) = parse_date(text) else {
        return Ok(None);
    };
    let timezone = project_timezone(client, project_id).await?;
    Ok(Some(plane_db::hooks::intake_issue::project_day_start(
        day, timezone,
    )))
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return None;
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// `self.initial_data.get(key)`.
fn initial_value(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<Json>, ViewError> {
    Ok(match ctx.data()? {
        plane_drf::parsers::Data::Json(Json::Dict(object)) => object.get(key).cloned(),
        plane_drf::parsers::Data::Json(_) => None,
        plane_drf::parsers::Data::Form(query) => {
            query.get(key).map(|value| Json::Str(PyStr::new(value)))
        }
    })
}

/// `writable_data(serializer, data)` for the nested issue body
/// (`plane/utils/serializers.py:8-15`): drop the keys that are read-only
/// fields of `IssueSerializer`.
fn nested_writable_data(data: &Json, patch: bool) -> Result<String, ViewError> {
    const READ_ONLY: [&str; 9] = [
        "id",
        "workspace",
        "project",
        "created_by",
        "updated_by",
        "updated_at",
        "completed_at",
        "deleted_at",
        "archived_at",
    ];
    let Json::Dict(object) = data else {
        return Err(ViewError::Unknown(
            "AttributeError: the nested issue data is not a mapping".to_owned(),
        ));
    };
    let mut kept = PyDict::new();
    for (key, value) in object.iter() {
        let name = key.as_str().unwrap_or_default();
        if READ_ONLY.contains(&name) || name == "sequence_id" || (patch && name == "is_draft") {
            continue;
        }
        kept.insert(key.clone(), value.clone());
    }
    issues::dumps(&Json::Dict(kept))
}

/// `Response({"error": …})` with a bare key.
fn plain_error(status: u16, message: &str) -> ViewResponse {
    write::error_body(status, message)
}

fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

fn is_integrity(error: &tokio_postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
}

fn map_issue_hook(error: plane_db::hooks::issue::IssueHookError) -> ViewError {
    match error {
        plane_db::hooks::issue::IssueHookError::ProjectNotFound => ViewError::ObjectDoesNotExist,
        other => ViewError::Unknown(other.to_string()),
    }
}
