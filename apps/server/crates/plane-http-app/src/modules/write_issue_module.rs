// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ModuleIssueViewSet`'s write actions (`plane/app/views/module/issue.py`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/modules/{module_id}/issues/` (106) | `create_module_issues` | `:222-254` |
//! | `POST …/issues/{issue_id}/modules/` (105) | `create_issue_modules` | `:256-319` |
//! | `DELETE …/modules/{module_id}/issues/{issue_id}/` (107) | `destroy` | `:321-356` |
//!
//! `PUT`/`PATCH` on 107 (and `GET` on the detail) are refused before dispatch
//! by `BaseViewSet`'s guard; `modules/mod.rs` registers the guard.
//!
//! Route 105's path is `/projects/{project_id}/issues/{issue_id}/modules/`,
//! which `deploy/route-groups.yaml` assigns to the **issues** group (its
//! `/projects/*/issues/**` glob is the more specific one), so
//! `create_issue_modules` is registered from `issues/mod.rs` — the proxy flips
//! a group as a unit. The handler stays here, next to the other
//! `ModuleIssueViewSet` actions.
//!
//! ## create_module_issues (`:222-254`)
//!
//! 1. `issues = request.data.get("issues", [])`; a falsy value is
//!    `400 {"error": "Issues are required"}`. A non-list is coerced by
//!    `pk__in` (a string iterates its characters, a dict its keys) and fails
//!    the UUID coercion → 400 `Please provide valid detail`, or is a
//!    `TypeError` → 500.
//! 2. `Project.objects.get(pk=project_id)` (404), then
//!    `Module.objects.get(pk=module_id, project_id, workspace__slug)` (404).
//! 3. The issue ids are re-read through `Issue.issue_objects` — archived,
//!    draft, triage and cross-project ids are silently dropped — in the
//!    model's `-created_at` order. That order is the insert order and the
//!    activity order, so it is observable.
//! 4. `bulk_create(..., batch_size=10, ignore_conflicts=True)`: one atomic
//!    over all batches with `ON CONFLICT DO NOTHING`, so a re-add is silently
//!    skipped (201 again) and the whole call never rolls back. `bulk_create`
//!    bypasses `save()`, so the caller passes `workspace` and
//!    `created_by`/`updated_by` explicitly and each row gets its own `now`.
//! 5. One `module.activity.created` per issue *after* the insert, with the
//!    issue id as a **str** and `project_id` as a **UUID**
//!    (`TASKS.md` row 58). A re-add publishes them again.
//! 6. `201 {"message": "success"}`.
//!
//! ## create_issue_modules (`:256-319`)
//!
//! 1. `modules` and `removed_modules`, both defaulting to `[]` and neither
//!    validated: `modules` reaches `pk__in` (characters of a string, keys of a
//!    dict, `TypeError` for a number), `removed_modules` is iterated
//!    (`TypeError` for `null`/a number) and each entry is UUID-coerced by the
//!    `filter(module_id=…)` inside the loop — so entries before a bad one are
//!    already published and deleted.
//! 2. `Project.objects.get` (404), then `Issue.objects.get(pk, project_id,
//!    workspace__slug)` — the **soft-delete** manager, so an archived/draft/
//!    triage issue is reachable — 404 otherwise.
//! 3. Only modules of the URL workspace+project survive; their ids are the
//!    `-created_at` order and are stringified.
//! 4. When non-empty: the same `ignore_conflicts` insert, then one
//!    `module.activity.created` per module with `issue_id`/`project_id` as
//!    **UUID** objects (`TASKS.md` row 59).
//! 5. For every `removed_modules` entry, **in request order**: a
//!    `module.activity.deleted` is published *before* the delete, even when no
//!    row matches (`module_name: null`); both ids are strings there, and
//!    `requested_data` stringifies the raw body value (row 60). Then the H04
//!    queryset delete — a bare `deleted_at` update, no `updated_at`, no
//!    cascade task. A repeat removal publishes again and updates nothing.
//! 6. `201 {"message": "success"}`.
//!
//! ## destroy (`:321-356`)
//!
//! Idempotent: no live `ModuleIssue` row answers `204` with **no** activity.
//! Otherwise `module.activity.deleted` (strings, `TASKS.md` row 61) then H04.
//! The module name is read through a join without the manager's
//! `deleted_at` filter, because Python's `select_related("module")` does not
//! apply it (`django/db/models/fields/related_descriptors.py`).

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks::issue_m2m::{MembershipRow, insert_membership_ignoring_conflicts};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json;
use uuid::Uuid;

use super::write_issue_queries as queries;
use crate::common::{self, jobs};
use crate::issues::write_sub_queries::{RawField, base_host, django_json, raw_field};
use crate::write;

// ---------------------------------------------------------------------------
// 106 — POST …/modules/{module_id}/issues/
// ---------------------------------------------------------------------------

/// `ModuleIssueViewSet.create_module_issues` (`module/issue.py:222-254`).
pub fn create_module_issues<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();

        // `issues = request.data.get("issues", [])`; `if not issues` (`:223-225`).
        let Some(requested) = requested_issue_ids(ctx)? else {
            return Ok(issues_required());
        };

        // `Project.objects.get(pk=project_id)` (`:226`).
        let workspace_id = project_workspace(client, project_id).await?;
        // `Module.objects.get(pk=module_id, project_id, workspace__slug)` (`:228`).
        require_module(client, &slug, project_id, module_id).await?;

        // `Issue.issue_objects.filter(...).values_list("id", flat=True)`
        // (`:230-236`), in `-created_at` order.
        let issue_ids = live_issue_ids(client, &slug, project_id, &requested).await?;

        // `bulk_create(..., batch_size=10, ignore_conflicts=True)` (`:238-252`).
        let rows: Vec<MembershipRow> = issue_ids
            .iter()
            .map(|issue_id| {
                membership_row(now, project_id, workspace_id, user, *issue_id, module_id)
            })
            .collect();
        insert_membership_ignoring_conflicts(client, "module_issues", "module_id", &rows, now)
            .await?;

        // One `module.activity.created` per issue, in the same order
        // (`:239-253`). `issue_id` is `str(issue)`, `project_id` a UUID.
        let requested_data = module_id_activity(&module_id.to_string())?;
        for issue_id in &issue_ids {
            publish(
                ctx,
                "module.activity.created",
                &requested_data,
                None,
                TaskArg::str(issue_id.to_string()),
                TaskArg::uuid(project_id),
                user,
                now,
            )
            .await?;
        }

        Ok(success())
    })
}

// ---------------------------------------------------------------------------
// 105 — POST …/issues/{issue_id}/modules/
// ---------------------------------------------------------------------------

/// `ModuleIssueViewSet.create_issue_modules` (`module/issue.py:256-319`).
pub fn create_issue_modules<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();
        let data = ctx.data()?;

        // `modules = request.data.get("modules", [])` (`:258`): no truthiness
        // check, `pk__in` coerces it when the queryset runs.
        let module_ids = coerced_module_ids(data)?;
        // `removed_modules = request.data.get("removed_modules", [])` (`:259`).
        let removed = removed_modules(data)?;

        // `Project.objects.get(pk=project_id)` (`:260`).
        let workspace_id = project_workspace(client, project_id).await?;
        // `Issue.objects.get(pk=issue_id, project_id, workspace__slug)` — the
        // soft-delete manager, so no `issue_objects` exclusions (`:263`).
        require_issue(client, &slug, project_id, issue_id).await?;

        // `Module.objects.filter(pk__in=modules, …).values_list("id", flat=True)`,
        // stringified (`:265-273`).
        let linked = in_project_modules(client, &slug, project_id, &module_ids).await?;

        if !linked.is_empty() {
            let rows: Vec<MembershipRow> = linked
                .iter()
                .map(|module_id| {
                    membership_row(now, project_id, workspace_id, user, issue_id, *module_id)
                })
                .collect();
            insert_membership_ignoring_conflicts(client, "module_issues", "module_id", &rows, now)
                .await?;

            // `issue_id`/`project_id` are UUID objects here (`TASKS.md` row 59).
            for module_id in &linked {
                let requested_data = module_id_activity(&module_id.to_string())?;
                publish(
                    ctx,
                    "module.activity.created",
                    &requested_data,
                    None,
                    TaskArg::uuid(issue_id),
                    TaskArg::uuid(project_id),
                    user,
                    now,
                )
                .await?;
            }
        }

        // The removal loop (`:293-317`): activity first, then the H04 delete,
        // one entry at a time.
        for entry in removed {
            let (module_id, raw_text) = entry.coerce()?;
            let name = module_name_for(client, &slug, project_id, module_id, issue_id).await?;
            let requested_data = module_id_activity(&raw_text)?;
            publish(
                ctx,
                "module.activity.deleted",
                &requested_data,
                Some(module_name_instance(name.as_deref())?),
                TaskArg::str(issue_id.to_string()),
                TaskArg::str(project_id.to_string()),
                user,
                now,
            )
            .await?;

            // `module_issue.delete()` — H04 (`:315`).
            let statement = queries::SOFT_DELETE_MODULE_ISSUE.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[&write::now(), &slug, &project_id, &module_id, &issue_id],
                )
                .await?;
        }

        Ok(success())
    })
}

// ---------------------------------------------------------------------------
// 107 — DELETE …/modules/{module_id}/issues/{issue_id}/
// ---------------------------------------------------------------------------

/// `ModuleIssueViewSet.destroy` (`module/issue.py:321-356`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();

        // `existing = module_issue.select_related("module").first()` (`:330`).
        let Some(name) = module_name_for(client, &slug, project_id, module_id, issue_id).await?
        else {
            // Already removed (a repeated delete): nothing to publish.
            return Ok(ViewResponse::empty(204));
        };

        let requested_data = module_id_activity(&module_id.to_string())?;
        publish(
            ctx,
            "module.activity.deleted",
            &requested_data,
            Some(module_name_instance(Some(&name))?),
            TaskArg::str(issue_id.to_string()),
            TaskArg::str(project_id.to_string()),
            user,
            now,
        )
        .await?;

        // `module_issue.delete()` — H04 (`:355`).
        let statement = queries::SOFT_DELETE_MODULE_ISSUE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[&now, &slug, &project_id, &module_id, &issue_id],
            )
            .await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    super::base::uuid_kwarg(ctx, name)
}

/// `{"message": "success"}` with `201` (DRF renders it compact).
fn success() -> ViewResponse {
    let body = Json::object(vec![("message", Json::string("success"))]);
    ViewResponse::raw(201, APPLICATION_JSON, body.to_drf())
}

/// `{"error": "Issues are required"}` with `400`.
fn issues_required() -> ViewResponse {
    let body = Json::object(vec![("error", Json::string("Issues are required"))]);
    ViewResponse::raw(400, APPLICATION_JSON, body.to_drf())
}

/// One through row; `updated_by` is the caller because `bulk_create` bypasses
/// `BaseModel.save` (`:238-252`, `:275-289`).
fn membership_row(
    _now: DateTime<Utc>,
    project_id: Uuid,
    workspace_id: Uuid,
    user: Uuid,
    issue_id: Uuid,
    module_id: Uuid,
) -> MembershipRow {
    MembershipRow {
        id: Uuid::new_v4(),
        project_id,
        workspace_id,
        created_by_id: Some(user),
        updated_by_id: Some(user),
        issue_id,
        other_id: module_id,
    }
}

/// `json.dumps({"module_id": str(module_id)})` — the text form of the id, as
/// the call site stringifies it.
fn module_id_activity(module_id: &str) -> Result<TaskArg, ViewError> {
    let body = Json::object(vec![("module_id", Json::string(module_id))]);
    Ok(TaskArg::json_text(django_json(&body)?))
}

/// `json.dumps({"module_name": name})` for the delete activities.
fn module_name_instance(name: Option<&str>) -> Result<TaskArg, ViewError> {
    let body = Json::object(vec![("module_name", name.map_or(Json::Null, Json::string))]);
    Ok(TaskArg::json_text(django_json(&body)?))
}

/// `Project.objects.get(pk=project_id).workspace_id` (`:226`, `:260`).
async fn project_workspace(client: &Client, project_id: Uuid) -> Result<Uuid, ViewError> {
    let statement = queries::PROJECT_WORKSPACE.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

/// `Module.objects.get(pk=module_id, project_id=…, workspace__slug=…)`.
async fn require_module(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    module_id: Uuid,
) -> Result<(), ViewError> {
    let statement = queries::MODULE_IN_PROJECT.prepare(client).await?;
    let exists: bool = client
        .query_one(&statement, &[&module_id, &project_id, &slug])
        .await?
        .get(0);
    if exists {
        Ok(())
    } else {
        Err(ViewError::ObjectDoesNotExist)
    }
}

/// `Issue.objects.get(pk=issue_id, project_id=…, workspace__slug=…)` — the
/// soft-delete manager only (`:263`).
async fn require_issue(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> Result<(), ViewError> {
    let statement = crate::issues::write_sub_queries::ISSUE_ANY_IN_PROJECT
        .prepare(client)
        .await?;
    let exists: bool = client
        .query_one(&statement, &[&issue_id, &project_id, &slug])
        .await?
        .get(0);
    if exists {
        Ok(())
    } else {
        Err(ViewError::ObjectDoesNotExist)
    }
}

/// The `Issue.issue_objects` id list (`:230-236`).
async fn live_issue_ids(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    requested: &[Uuid],
) -> Result<Vec<Uuid>, ViewError> {
    let statement = queries::MODULE_ISSUE_ISSUE_IDS.prepare(client).await?;
    let rows = client
        .query(&statement, &[&slug, &project_id, &requested])
        .await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// The in-project module ids (`:265-273`).
async fn in_project_modules(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    requested: &[Uuid],
) -> Result<Vec<Uuid>, ViewError> {
    let statement = queries::MODULE_ISSUE_MODULE_IDS.prepare(client).await?;
    let rows = client
        .query(&statement, &[&requested, &project_id, &slug])
        .await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

/// `module_issue.first().module.name`, `None` when no live row matches
/// (`:302-312`) — and, with `select_related`, the destroy guard (`:336`).
async fn module_name_for(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    module_id: Uuid,
    issue_id: Uuid,
) -> Result<Option<String>, ViewError> {
    let statement = queries::MODULE_ISSUE_FIRST.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&slug, &project_id, &module_id, &issue_id])
        .await?;
    Ok(row.map(|row| row.get(1)))
}

/// `issue_activity.delay(...)` in the View shape (`notification=True`,
/// `origin` last), the shape all five module call sites use. `current_instance`
/// is the caller's `TaskArg`: a `json.dumps` string for the delete sites and
/// `None` (the task keyword is `None`, not a JSON `null` string) for the
/// create sites.
#[allow(clippy::too_many_arguments)]
async fn publish(
    ctx: &ViewCtx<'_>,
    activity_type: &str,
    requested_data: &TaskArg,
    current_instance: Option<TaskArg>,
    issue_id: TaskArg,
    project_id: TaskArg,
    user: Uuid,
    epoch: DateTime<Utc>,
) -> Result<(), ViewError> {
    let current_instance = current_instance.unwrap_or(TaskArg::Null);
    let base = IssueActivityBase::new(
        activity_type,
        requested_data.clone(),
        user.to_string(),
        issue_id,
        project_id,
        current_instance,
        epoch.timestamp(),
    );
    let message = tasks::issue_activity(IssueActivityCall::view(base, base_host(ctx)?));
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// `request.data.get(...)` shapes
// ---------------------------------------------------------------------------

/// `issues` from the body, coerced the way `pk__in` coerces it.
///
/// `Ok(None)` is Python's falsy value (the view's 400): absent, `null`, an
/// empty list/dict/string, `0` or `false`. Everything non-empty either coerces
/// (list elements, dict keys) or is the ORM's exception: a bad UUID is 400
/// `Validation`; a scalar that is not iterable is a `TypeError` → 500.
fn requested_issue_ids(ctx: &ViewCtx<'_>) -> Result<Option<Vec<Uuid>>, ViewError> {
    match raw_field(ctx.data()?, "issues") {
        RawField::Absent | RawField::Null => Ok(None),
        RawField::Text(text) => {
            if text.is_empty() {
                Ok(None)
            } else {
                // `pk__in="abc"` iterates the characters.
                Err(ViewError::Validation)
            }
        }
        RawField::Other(value) => coerce_iterable(value),
    }
}

/// `Module.objects.filter(pk__in=value)` (`:265-273`): the value is iterated
/// and each entry UUID-coerced in `In.get_prep_lookup`
/// (`django/db/models/lookups.py:306`). Absent/`null` is `pk__in=None` —
/// `_meta.get_field("pk").get_prep_value` is not reached, Python raises
/// `TypeError: 'NoneType' object is not iterable` → 500.
fn coerced_module_ids(data: &Data) -> Result<Vec<Uuid>, ViewError> {
    match raw_field(data, "modules") {
        RawField::Absent => Ok(Vec::new()),
        RawField::Null => Err(not_iterable("NoneType")),
        RawField::Text(text) => {
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        RawField::Other(value) => Ok(coerce_iterable(value)?.unwrap_or_default()),
    }
}

/// One entry of `removed_modules`, kept lazy so a bad entry only fails after
/// the entries before it were published and deleted.
enum RemovedModule<'a> {
    /// A list element (any JSON value).
    Value(&'a plane_serde::pyjson::Value),
    /// A dict key: Python iterates keys, which are always strings.
    Key(&'a str),
}

impl RemovedModule<'_> {
    /// `filter(module_id=value)`'s `UUIDField.get_prep_value` plus
    /// `str(value)` for `requested_data`.
    fn coerce(&self) -> Result<(Uuid, String), ViewError> {
        match self {
            Self::Value(value) => {
                let id = coerce_uuid(value)?;
                Ok((id, python_str(value)?))
            }
            Self::Key(text) => Ok((coerce_uuid_str(text)?, (*text).to_owned())),
        }
    }
}

/// `removed_modules = request.data.get("removed_modules", [])` iterated by the
/// view (`:295`).
fn removed_modules(data: &Data) -> Result<Vec<RemovedModule<'_>>, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match raw_field(data, "removed_modules") {
        RawField::Absent => Ok(Vec::new()),
        RawField::Null => Err(not_iterable("NoneType")),
        RawField::Text(text) => {
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                // A string iterates its characters; no character is a UUID.
                Err(ViewError::Validation)
            }
        }
        RawField::Other(Py::List(items)) => Ok(items.iter().map(RemovedModule::Value).collect()),
        RawField::Other(Py::Dict(object)) => Ok(object
            .iter()
            .filter_map(|(key, _)| key.as_str().map(RemovedModule::Key))
            .collect()),
        RawField::Other(Py::Int(_)) => Err(not_iterable("int")),
        RawField::Other(Py::Bool(_)) => Err(not_iterable("bool")),
        RawField::Other(Py::Float(_)) => Err(not_iterable("float")),
        // `raw_field` maps a JSON `null` to `RawField::Null`; unreachable.
        RawField::Other(Py::Str(_) | Py::Null) => Err(ViewError::Validation),
    }
}

fn not_iterable(name: &str) -> ViewError {
    ViewError::Unknown(format!("TypeError: '{name}' object is not iterable"))
}

/// `for value in value` + `UUIDField.get_prep_value` per element.
fn coerce_iterable(value: &plane_serde::pyjson::Value) -> Result<Option<Vec<Uuid>>, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::List(items) => {
            if items.is_empty() {
                return Ok(None);
            }
            items
                .iter()
                .map(coerce_uuid)
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        Py::Dict(object) => {
            if object.iter().next().is_none() {
                return Ok(None);
            }
            object
                .iter()
                .map(|(key, _)| {
                    key.as_str()
                        .map_or(Err(ViewError::Validation), coerce_uuid_str)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        Py::Int(number) => {
            if number.as_i64() == Some(0) {
                Ok(None)
            } else {
                Err(not_iterable("int"))
            }
        }
        Py::Bool(false) => Ok(None),
        Py::Bool(true) => Err(not_iterable("bool")),
        Py::Float(number) if *number == 0.0 => Ok(None),
        Py::Float(_) => Err(not_iterable("float")),
        Py::Null => Ok(None),
        // A string that reached this helper is non-empty; the characters fail
        // the UUID coercion.
        Py::Str(_) => Err(ViewError::Validation),
    }
}

/// `UUIDField.get_prep_value` for one value (`django/db/models/fields/
/// __init__.py`, `UUIDField.to_python`): a string is hex, an integer is the
/// `int` form (a `bool` is an `int` in Python), anything else fails.
fn coerce_uuid(value: &plane_serde::pyjson::Value) -> Result<Uuid, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Str(text) => match text.as_str() {
            Some(text) => coerce_uuid_str(text),
            None => Err(ViewError::Validation),
        },
        Py::Int(number) => number
            .as_i64()
            .and_then(|value| u128::try_from(value).ok())
            .map(Uuid::from_u128)
            .ok_or(ViewError::Validation),
        Py::Bool(flag) => Ok(Uuid::from_u128(u128::from(*flag))),
        _ => Err(ViewError::Validation),
    }
}

fn coerce_uuid_str(text: &str) -> Result<Uuid, ViewError> {
    Uuid::parse_str(text).map_err(|_| ViewError::Validation)
}

/// Python `str(value)` for a scalar: `"5"`, `"True"`, `"1.5"`.
fn python_str(value: &plane_serde::pyjson::Value) -> Result<String, ViewError> {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Str(text) => text
            .as_str()
            .map(str::to_owned)
            .ok_or(ViewError::Validation),
        Py::Int(number) => Ok(match number.as_i64() {
            Some(value) => value.to_string(),
            None => number.to_string(),
        }),
        Py::Bool(true) => Ok("True".to_owned()),
        Py::Bool(false) => Ok("False".to_owned()),
        Py::Float(number) => Ok(python_float(*number)),
        _ => Err(ViewError::Validation),
    }
}

/// Python's `repr(float)` for the values JSON can carry.
fn python_float(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}
