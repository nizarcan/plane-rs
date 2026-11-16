// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceDraftIssueViewSet`'s write actions
//! (`plane/app/views/workspace/draft.py:126-243`).
//!
//! | Route | Action |
//! | --- | --- |
//! | `POST /api/workspaces/{slug}/draft-issues/` | [`create`] |
//! | `PATCH /api/workspaces/{slug}/draft-issues/{pk}/` | [`partial_update`] |
//! | `DELETE /api/workspaces/{slug}/draft-issues/{pk}/` | [`destroy`] |
//!
//! Draft creation/update uses `DraftIssueCreateSerializer` (the generated
//! `draft_issue_create_serializer::validate` plus the project-scoped checks the
//! serializer's `validate()` adds), then `DraftIssue.save`
//! (`hooks::draft_issue`). The create response is the view's `.values(...)`
//! projection, not a serializer.

use chrono::Utc;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_db::hooks::draft_issue;
use plane_db::hooks::issue_m2m;
use plane_db::models::draft::DraftIssue;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{IssueActivityBase, IssueActivityCall};
use plane_query::json::Json as Doc;
use plane_serde::pyjson::{PyStr, Value as Json, Value as PyValue};
use plane_serde::values::DjangoDateTime;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::issues::write_draft_queries as queries;
use crate::issues::{write_issue, write_support};
use crate::write::{self, Serializer, description};

/// `POST /api/workspaces/{slug}/draft-issues/`.
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;
        let client = client(ctx)?;
        let workspace = lookup::workspace_by_slug(client, slug).await?;

        let project_id =
            data_string(ctx, "project_id").and_then(|value| Uuid::parse_str(&value).ok());
        if let Some(project_id) = project_id
            && !can_write_project(client, user, workspace.id, project_id).await?
        {
            return Ok(write::error_body(
                403,
                "You don't have the required permissions.",
            ));
        }

        let mut serializer = Serializer::new(ctx, false)?;
        write::generated::draft_issue_create_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        let description_html = match validate_description(values.text("description_html")) {
            Ok(value) => value,
            Err(response) => return Ok(response),
        };
        let description_json = values
            .json("description_json")
            .cloned()
            .unwrap_or_else(|| JsonText::new("{}"));
        let (start_date, target_date) = (values.date("start_date"), values.date("target_date"));
        if let (Some(start), Some(target)) = (start_date, target_date)
            && start > target
        {
            return Ok(non_field("Start date cannot exceed target date"));
        }

        let state = value_uuid(&values, "state_id").or_else(|| value_uuid(&values, "state"));
        let parent = value_uuid(&values, "parent_id").or_else(|| value_uuid(&values, "parent"));
        if let Some(state) = state
            && let Some(project_id) = project_id
            && client
                .query_opt(queries::STATE_IN_PROJECT, &[&state, &project_id])
                .await?
                .is_none()
        {
            return Ok(non_field("State is not valid please pass a valid state_id"));
        }
        if let Some(parent) = parent
            && let Some(project_id) = project_id
            && client
                .query_opt(queries::ISSUE_IN_PROJECT, &[&parent, &project_id])
                .await?
                .is_none()
        {
            return Ok(non_field(
                "Parent is not valid issue_id please pass a valid issue_id",
            ));
        }
        if let Some(estimate) = values.uuid("estimate_point")
            && let Some(project_id) = project_id
            && client
                .query_opt(
                    queries::ESTIMATE_POINT_IN_PROJECT,
                    &[&estimate, &project_id],
                )
                .await?
                .is_none()
        {
            return Ok(non_field(
                "Estimate point is not valid please pass a valid estimate_point_id",
            ));
        }

        let cycle_id = data_uuid(ctx, "cycle_id")?;
        let module_ids = data_uuid_list(ctx, "module_ids")?;
        if let Some(project_id) = project_id {
            if let Some(cycle_id) = cycle_id
                && client
                    .query_opt(
                        queries::CYCLE_IN_PROJECT,
                        &[&cycle_id, &project_id, &workspace.id],
                    )
                    .await?
                    .is_none()
            {
                return Ok(non_field("Cycle is not valid please pass a valid cycle_id"));
            }
            if !module_ids.is_empty() {
                let count: i64 = client
                    .query_one(
                        queries::MODULES_IN_PROJECT,
                        &[&module_ids, &project_id, &workspace.id],
                    )
                    .await?
                    .get(0);
                if count as usize != module_ids.len() {
                    return Ok(non_field(
                        "Modules are not valid please pass valid module_ids",
                    ));
                }
            }
        }

        let assignees =
            filter_assignees(client, project_id, values.uuid_list("assignee_ids")).await?;
        let labels = filter_labels(client, project_id, values.uuid_list("label_ids")).await?;

        let now = write::now();
        let model = DraftIssue {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: values.datetime("deleted_at"),
            id: Uuid::new_v4(),
            workspace_id: workspace.id,
            project_id,
            parent_id: parent,
            state_id: None,
            estimate_point_id: values.uuid("estimate_point"),
            name: values.text("name").map(str::to_owned),
            description_json,
            description_html,
            description_stripped: None,
            description_binary: None,
            priority: values.text("priority").unwrap_or("none").to_owned(),
            start_date,
            target_date,
            sort_order: values.float("sort_order").unwrap_or(65535.0),
            completed_at: values.datetime("completed_at"),
            external_source: values.text("external_source").map(str::to_owned),
            external_id: values.text("external_id").map(str::to_owned),
            type_id: values.uuid("type"),
        };
        let prepared = draft_issue::before_create(
            client,
            project_id,
            state,
            &model.description_html,
            model.sort_order,
            now,
        )
        .await
        .map_err(ViewError::from)?;
        let model = DraftIssue {
            state_id: prepared.state_id,
            completed_at: prepared.completed_at,
            description_stripped: prepared.description_stripped,
            sort_order: prepared.sort_order,
            ..model
        };
        insert_draft(client, &model).await?;
        insert_m2m(
            client,
            &model,
            &assignees,
            &labels,
            cycle_id,
            &module_ids,
            now,
        )
        .await?;

        let body = response_body(
            client,
            model.id,
            &ctx.timezone.copied().unwrap_or(chrono_tz::UTC),
        )
        .await?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `PATCH /api/workspaces/{slug}/draft-issues/{pk}/`.
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;
        let pk = Uuid::parse_str(ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let row = client
            .query_opt(queries::DRAFT_BY_ID_FOR_USER, &[&pk, &workspace.id, &user])
            .await?
            .ok_or_else(|| ViewError::Unknown("Issue not found".to_string()))?;
        let mut model = DraftIssue::from_row(&row, 0)?;

        let body_project =
            data_string(ctx, "project_id").and_then(|value| Uuid::parse_str(&value).ok());
        for project_id in [model.project_id, body_project].into_iter().flatten() {
            if !can_write_project(client, user, workspace.id, project_id).await? {
                return Ok(write::error_body(
                    403,
                    "You don't have the required permissions.",
                ));
            }
        }

        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::draft_issue_create_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        let description_html = match values.contains("description_html") {
            true => match validate_description(values.text("description_html")) {
                Ok(value) => Some(value),
                Err(response) => return Ok(response),
            },
            false => None,
        };
        let start_date = values.date("start_date").or(model.start_date);
        let target_date = values.date("target_date").or(model.target_date);
        if let (Some(start), Some(target)) = (start_date, target_date)
            && start > target
        {
            return Ok(non_field("Start date cannot exceed target date"));
        }

        let state = if values.contains("state") {
            value_uuid(&values, "state")
        } else if values.contains("state_id") {
            value_uuid(&values, "state_id")
        } else {
            model.state_id
        };
        let parent = if values.contains("parent") {
            value_uuid(&values, "parent")
        } else if values.contains("parent_id") {
            value_uuid(&values, "parent_id")
        } else {
            model.parent_id
        };
        if let Some(state) = state
            && let Some(project_id) = model.project_id
            && client
                .query_opt(queries::STATE_IN_PROJECT, &[&state, &project_id])
                .await?
                .is_none()
        {
            return Ok(non_field("State is not valid please pass a valid state_id"));
        }
        if let Some(parent) = parent
            && let Some(project_id) = model.project_id
            && client
                .query_opt(queries::ISSUE_IN_PROJECT, &[&parent, &project_id])
                .await?
                .is_none()
        {
            return Ok(non_field(
                "Parent is not valid issue_id please pass a valid issue_id",
            ));
        }

        // `DraftIssueCreateSerializer.validate`'s `check_cycle_and_modules`
        // on PATCH: the instance's own project/workspace are the scope.
        let cycle = data_uuid(ctx, "cycle_id")?;
        if data_has(ctx, "cycle_id")
            && let Some(cycle) = cycle
            && client
                .query_opt(
                    queries::CYCLE_IN_PROJECT,
                    &[&cycle, &model.project_id, &model.workspace_id],
                )
                .await?
                .is_none()
        {
            return Ok(non_field("Cycle is not valid please pass a valid cycle_id"));
        }
        let modules = data_uuid_list(ctx, "module_ids")?;
        if data_has(ctx, "module_ids") && !modules.is_empty() {
            let count: i64 = client
                .query_one(
                    queries::MODULES_IN_PROJECT,
                    &[&modules, &model.project_id, &model.workspace_id],
                )
                .await?
                .get(0);
            if count as usize != modules.len() {
                return Ok(non_field(
                    "Modules are not valid please pass valid module_ids",
                ));
            }
        }

        let now = write::now();
        if let Some(description_html) = description_html {
            model.description_html = description_html;
        }
        if let Some(description_json) = values.json("description_json") {
            model.description_json = description_json.clone();
        }
        if let Some(name) = values.text("name") {
            model.name = Some(name.to_owned());
        }
        if let Some(priority) = values.text("priority") {
            model.priority = priority.to_owned();
        }
        if let Some(sort_order) = values.float("sort_order") {
            model.sort_order = sort_order;
        }
        if let Some(estimate) = values.uuid("estimate_point") {
            model.estimate_point_id = Some(estimate);
        }
        if let Some(kind) = values.uuid("type") {
            model.type_id = Some(kind);
        }
        if values.contains("deleted_at") {
            model.deleted_at = values.datetime("deleted_at");
        }
        model.start_date = start_date;
        model.target_date = target_date;
        model.external_source = values
            .text("external_source")
            .map(str::to_owned)
            .or(model.external_source);
        model.external_id = values
            .text("external_id")
            .map(str::to_owned)
            .or(model.external_id);
        model.parent_id = parent;

        let prepared = draft_issue::before_update(
            client,
            model.project_id,
            state,
            &model.description_html,
            now,
        )
        .await
        .map_err(ViewError::from)?;
        model.state_id = prepared.state_id;
        model.completed_at = prepared.completed_at;
        model.description_stripped = prepared.description_stripped;
        model.updated_at = now;
        model.updated_by_id = Some(user);
        update_draft(client, &model).await?;

        if values.contains("assignee_ids") {
            replace_assignees(
                client,
                &model,
                &filter_assignees(client, model.project_id, values.uuid_list("assignee_ids"))
                    .await?,
                now,
            )
            .await?;
        }
        if values.contains("label_ids") {
            replace_labels(
                client,
                &model,
                &filter_labels(client, model.project_id, values.uuid_list("label_ids")).await?,
                now,
            )
            .await?;
        }
        if data_has(ctx, "cycle_id") {
            client
                .execute(queries::SOFT_DELETE_CYCLES, &[&now, &model.id])
                .await?;
            if let Some(cycle) = cycle {
                insert_cycle(client, &model, cycle, now).await?;
            }
        }
        if data_has(ctx, "module_ids") {
            client
                .execute(queries::SOFT_DELETE_MODULES, &[&now, &model.id])
                .await?;
            insert_modules(client, &model, &modules, now).await?;
        }
        Ok(ViewResponse::empty(204))
    })
}

/// `DELETE /api/workspaces/{slug}/draft-issues/{pk}/`.
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx.user.id().ok_or(ViewError::Api(
            plane_core::error::ApiError::NotAuthenticated,
        ))?;
        let pk = Uuid::parse_str(ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;
        let workspace = lookup::workspace_by_slug(client, slug).await?;
        let row = client
            .query_opt(queries::DRAFT_BY_ID, &[&pk, &workspace.id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let mut draft = DraftIssue::from_row(&row, 0)?;
        let now = write::now();
        // `SoftDeleteModel.delete` calls the model's full `save()`, so
        // `DraftIssue.save` re-runs (state/completed_at/strip) before the row
        // is written; it is not the column-only `write::soft_delete`.
        draft.deleted_at = Some(now);
        draft.updated_at = now;
        draft.updated_by_id = Some(user);
        let prepared = draft_issue::before_update(
            client,
            draft.project_id,
            draft.state_id,
            &draft.description_html,
            now,
        )
        .await
        .map_err(ViewError::from)?;
        draft.completed_at = prepared.completed_at;
        draft.description_stripped = prepared.description_stripped;
        update_draft(client, &draft).await?;
        write::publish_soft_delete_cascade(ctx, "db", "draftissue", draft.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `POST /api/workspaces/{slug}/draft-to-issue/{draft_id}/`
/// (`WorkspaceDraftIssueViewSet.create_draft_to_issue`,
/// `plane/app/views/workspace/draft.py:243-371`).
///
/// The issue itself is `IssueCreateSerializer.save()`, i.e.
/// [`super::write_issue::prepare_create`] + [`super::write_issue::
/// write_created_issue`] — the same advisory-lock sequence, `sort_order` and
/// assignee/label through rows as `IssueViewSet.create`. What is specific to
/// this route:
///
/// * the draft lookup is creator-scoped and its 404 is the hand-built
///   `{"error": "The required object does not exist."}`;
/// * a draft without a project is a 400 *before* the permission re-check, and
///   the caller must be able to write the draft's project (403 otherwise);
/// * `cycle_id`/`module_ids` are read from the **raw** body (they are not
///   serializer fields) and are forwarded to `CycleIssue`/`ModuleIssue`;
/// * the tasks are `issue.activity.created`, then `cycle.activity.created`,
///   then one `module.activity.created` per module — **not** the
///   `model_activity`/`issue_description_version_task` pair issue create
///   publishes;
/// * the draft's description file assets are re-pointed at the new issue and
///   the draft is soft-deleted (a full row save followed by the cascade task);
/// * the 201 body is `IssueCreateSerializer.data` — the 34 readable fields
///   plus the two raw `assignee_ids`/`label_ids` arrays — not the flat
///   `.values(...)` row issue create answers with.
///
/// ## Transaction boundaries
///
/// The request is autocommit (`ATOMIC_REQUESTS` is unset and nothing here opens
/// an atomic), so each of these commits independently:
///
/// 1. `Issue.save()`'s `transaction.atomic()`: advisory lock, sequence,
///    sort order, the `issues` INSERT and the `issue_sequences` INSERT — a
///    failure rolls all of that back;
/// 2. the assignee `bulk_create` and the label `bulk_create`, each one
///    outermost `BEGIN … COMMIT` over all its batches; the serializer's
///    `except IntegrityError: pass` swallows a unique violation after that
///    call's own rollback, and any other error propagates as the family's 400;
/// 3. `CycleIssue.objects.create` — one autocommit INSERT;
/// 4. `ModuleIssue.objects.bulk_create` — one outermost atomic over every
///    batch; a duplicate module (the list is not de-duplicated) rolls every
///    module row back and propagates the `IntegrityError` as a 400, **after**
///    the issue, its through rows, the cycle row and the first two activity
///    messages are already committed;
/// 5. the `file_assets` UPDATE and the draft's soft delete.
///
/// So a request that fails at step 4 leaves a committed issue behind: that
/// partial write is the Python's behaviour and the scenario pins it.
pub fn create_draft_to_issue<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let draft_id = Uuid::parse_str(ctx.kwarg("draft_id").ok_or(ViewError::Key)?)
            .map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;

        // `self.get_queryset().filter(pk=draft_id, created_by=request.user).first()`
        // (`draft.py:247`).
        let row = client
            .query_opt(queries::DRAFT_TO_ISSUE, &[&draft_id, &slug, &user])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let mut draft = DraftIssue::from_row(&row, 0)?;
        let workspace_id: Uuid = row.get(DraftIssue::COLUMN_COUNT);
        // `draft_issue.project.workspace_id` / `.default_assignee_id`.
        let project_workspace_id: Option<Uuid> = row.get(DraftIssue::COLUMN_COUNT + 1);
        let default_assignee_id: Option<Uuid> = row.get(DraftIssue::COLUMN_COUNT + 2);

        let Some(project_id) = draft.project_id else {
            return Ok(write::error_body(
                400,
                "Project is required to create an issue.",
            ));
        };
        if !can_write_project(client, user, workspace_id, project_id).await? {
            return Ok(write::error_body(
                403,
                "You don't have the required permissions.",
            ));
        }

        // `DraftIssueCreateSerializer.check_cycle_and_modules(request.data.get(
        // "cycle_id"), request.data.get("module_ids"), …)` (`draft.py:268-273`).
        // A non-UUID value reaches a queryset filter and raises Django's
        // `ValidationError`, which the family `handle_exception` maps to
        // `{"error": "Please provide valid detail"}` (`views/base.py:216-220`);
        // a well-formed value of another project is the DRF `ValidationError`
        // string, i.e. `{"detail": …}`.
        let cycle_raw = raw_body_value(ctx, "cycle_id")?;
        let cycle_id = match cycle_raw
            .as_ref()
            .filter(|value| write_support::truthy(value))
        {
            Some(value) => {
                let cycle_id = raw_uuid(value)?;
                if client
                    .query_opt(
                        queries::CYCLE_IN_PROJECT,
                        &[&cycle_id, &project_id, &draft.workspace_id],
                    )
                    .await?
                    .is_none()
                {
                    return Err(validation_detail(
                        "Cycle is not valid please pass a valid cycle_id",
                    ));
                }
                Some(cycle_id)
            }
            None => None,
        };
        let modules_raw = raw_body_value(ctx, "module_ids")?;
        let module_ids = match modules_raw
            .as_ref()
            .filter(|value| write_support::truthy(value))
        {
            Some(value) => {
                let module_ids = raw_uuid_list(value)?;
                let count: i64 = client
                    .query_one(
                        queries::MODULES_IN_PROJECT,
                        &[&module_ids, &project_id, &draft.workspace_id],
                    )
                    .await?
                    .get(0);
                // `len(set(module_ids))` counts the raw values, so two
                // spellings of one uuid are two entries against one row.
                let distinct = distinct_raw(value)?;
                if count as usize != distinct {
                    return Err(validation_detail(
                        "Modules are not valid please pass valid module_ids",
                    ));
                }
                Some(module_ids)
            }
            None => None,
        };

        // `IssueCreateSerializer(...).save()` with the draft's project, its
        // workspace and its default assignee (`draft.py:275-285`).
        let Some(project_workspace_id) = project_workspace_id else {
            return Err(ViewError::Unknown(
                "RelatedObjectDoesNotExist: DraftIssue has no project".to_owned(),
            ));
        };
        let prepared = write_issue::prepare_create(ctx, client, project_id, false).await?;
        let write_issue::Created { mut issue, extra } = match prepared {
            write_issue::PreparedCreate::Valid(created) => *created,
            write_issue::PreparedCreate::Invalid(response) => return Ok(response),
        };
        write_issue::write_created_issue(client, &mut issue, &extra, default_assignee_id, user)
            .await?;

        // `issue_activity.delay(…)` (`draft.py:287-297`): `requested_data` is
        // the whole raw body, `issue_id` the new issue's id as a string.
        let body = write_support::request_object(ctx)?;
        let origin = write_support::base_host(ctx)?;
        write_issue::publish_issue_activity(
            ctx,
            IssueActivityCall::view(
                write_issue::activity_base(
                    "issue.activity.created",
                    TaskArg::json_text_from(&body)
                        .map_err(|error| ViewError::Unknown(error.to_string()))?,
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    TaskArg::Null,
                ),
                origin.clone(),
            ),
        )
        .await?;

        // `CycleIssue.objects.create(...)` (`draft.py:299-307`) and its
        // activity. The created row's audit columns come from `crum`
        // (`BaseModel.save`), so `updated_by_id` is NULL even when the draft
        // had one.
        if let Some(cycle_id) = cycle_id {
            let created_at = write::now();
            let updated_at = write::now();
            let cycle_issue_id = Uuid::new_v4();
            client
                .execute(
                    queries::INSERT_CYCLE_ISSUE,
                    &[
                        &created_at,
                        &updated_at,
                        &Some(user),
                        &None::<Uuid>,
                        &cycle_issue_id,
                        &project_id,
                        &project_workspace_id,
                        &issue.id,
                        &cycle_id,
                    ],
                )
                .await?;

            // `json.dumps({"updated_cycle_issues": None,
            // "created_cycle_issues": serializers.serialize("json", [row])})`.
            // The inner string is Django's python serializer: `model`/`pk`/
            // `fields` in `_meta.local_fields` order, datetimes through
            // `DjangoJSONEncoder` (milliseconds, `Z`).
            let cycle_fields = Doc::object(vec![
                ("created_at", Doc::string(dje_datetime(created_at)?)),
                ("updated_at", Doc::string(dje_datetime(updated_at)?)),
                ("created_by", Doc::uuid(user)),
                ("updated_by", Doc::Null),
                ("deleted_at", Doc::Null),
                ("project", Doc::uuid(project_id)),
                ("workspace", Doc::uuid(project_workspace_id)),
                ("issue", Doc::uuid(issue.id)),
                ("cycle", Doc::uuid(cycle_id)),
            ]);
            let serialized = Doc::Array(vec![Doc::object(vec![
                ("model", Doc::string("db.cycleissue")),
                ("pk", Doc::uuid(cycle_issue_id)),
                ("fields", cycle_fields),
            ])]);
            let serialized = String::from_utf8(write_support::dje_bytes(&serialized)?)
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
            let current_instance = Doc::object(vec![
                ("updated_cycle_issues", Doc::Null),
                ("created_cycle_issues", Doc::string(serialized)),
            ]);
            write_issue::publish_issue_activity(
                ctx,
                IssueActivityCall::view(
                    IssueActivityBase::new(
                        "cycle.activity.created",
                        TaskArg::Null,
                        user.to_string(),
                        // `issue_id=None` is the literal `None`;
                        // `project_id=str(kwargs.get("project_id", None))` and
                        // this route has no `project_id` kwarg, so that one is
                        // the **string** `"None"` (`draft.py:313-314`).
                        TaskArg::Null,
                        TaskArg::str("None"),
                        TaskArg::json_text(dje_text(&current_instance)?),
                        Utc::now().timestamp(),
                    ),
                    origin.clone(),
                ),
            )
            .await?;
        }

        // `ModuleIssue.objects.bulk_create([...], batch_size=10)`
        // (`draft.py:326-341`): the audit ids are the draft's, copied
        // explicitly (no `save()`, so no `crum`), and the call is one atomic.
        if let Some(module_ids) = &module_ids {
            let rows: Vec<issue_m2m::MembershipRow> = module_ids
                .iter()
                .map(|module_id| issue_m2m::MembershipRow {
                    id: Uuid::new_v4(),
                    project_id,
                    workspace_id: draft.workspace_id,
                    created_by_id: draft.created_by_id,
                    updated_by_id: draft.updated_by_id,
                    issue_id: issue.id,
                    other_id: *module_id,
                })
                .collect();
            if let Err(error) = issue_m2m::insert_membership(
                client,
                "module_issues",
                "module_id",
                &rows,
                write::now(),
            )
            .await
            {
                // Django's `IntegrityError` (any SQLSTATE class 23, including a
                // deferred FK violation at COMMIT) is the family's 400; nothing
                // catches it at this call site.
                return Err(match &error {
                    plane_db::DbError::Postgres(error)
                        if error
                            .code()
                            .is_some_and(|code| code.code().starts_with("23")) =>
                    {
                        ViewError::Integrity
                    }
                    other => ViewError::Unknown(other.to_string()),
                });
            }

            // One `module.activity.created` per raw module id, in request
            // order (`draft.py:343-356`). `issue_id` is
            // `serializer.data["id"]` (a string, DRF renders the UUID) while
            // `project_id` is the raw `UUID` object — the two differ on the
            // wire and in `kwargsrepr`.
            for module_id in module_ids {
                let requested =
                    Doc::object(vec![("module_id", Doc::string(module_id.to_string()))]);
                write_issue::publish_issue_activity(
                    ctx,
                    IssueActivityCall::view(
                        IssueActivityBase::new(
                            "module.activity.created",
                            TaskArg::json_text(dje_text(&requested)?),
                            user.to_string(),
                            // `Serializer.data["id"]` for a
                            // `PrimaryKeyRelatedField` is the raw pk object
                            // (`PKOnlyObject.pk`), i.e. a `uuid.UUID`, not its
                            // string — unlike the issue activity above, which
                            // calls `str(...)`.
                            TaskArg::uuid(issue.id),
                            TaskArg::uuid(project_id),
                            TaskArg::Null,
                            Utc::now().timestamp(),
                        ),
                        origin.clone(),
                    ),
                )
                .await?;
            }
        }

        // `FileAsset.objects.filter(draft_issue_id=draft_id).update(...)`
        // (`draft.py:358-364`).
        client
            .execute(queries::REASSIGN_FILE_ASSETS, &[&issue.id, &draft_id])
            .await?;

        // `draft_issue.delete()` (`draft.py:366-367`): `SoftDeleteModel.delete`
        // sets `deleted_at`, then the full `DraftIssue.save()` re-runs the
        // state/`completed_at`/`description_stripped` hooks and writes every
        // column, and the cascade task is published.
        draft.deleted_at = Some(write::now());
        draft.updated_at = write::now();
        draft.updated_by_id = Some(user);
        let prepared = draft_issue::before_update(
            client,
            draft.project_id,
            draft.state_id,
            &draft.description_html,
            write::now(),
        )
        .await
        .map_err(ViewError::from)?;
        draft.completed_at = prepared.completed_at;
        draft.description_stripped = prepared.description_stripped;
        update_draft(client, &draft).await?;
        write::publish_soft_delete_cascade(ctx, "db", "draftissue", draft.id).await?;

        // `Response(serializer.data, 201)` (`draft.py:369`): the serializer's
        // `to_representation`, which reads `assignees`/`labels` back from the
        // through tables and overwrites `assignee_ids`/`label_ids` with the
        // **raw request values** (so a filtered-out assignee still appears).
        let assignees = relation_ids(client, queries::ISSUE_ASSIGNEE_IDS, issue.id).await?;
        let labels = relation_ids(client, queries::ISSUE_LABEL_IDS, issue.id).await?;
        let assignee_ids = raw_id_array(&assignee_raw(ctx, "assignee_ids")?);
        let label_ids = raw_id_array(&assignee_raw(ctx, "label_ids")?);
        let description_json = parse_json(&issue.description_json)?;
        let body = issue_create_representation(
            &issue,
            write_support::timezone(ctx),
            &assignees,
            &labels,
            description_json,
            assignee_ids,
            label_ids,
        );
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body.to_drf()))
    })
}

/// `raise serializers.ValidationError("…")` from the view.
///
/// `ValidationError.__init__` coerces a non-dict detail into a **list**
/// (`rest_framework/exceptions.py:148-161`), so the body is
/// `["<message>"]` — not the `{"detail": …}` a plain `APIException` would
/// produce.
fn validation_detail(message: &str) -> ViewError {
    ViewError::Api(ApiError::ValidationError(serde_json::json!([message])))
}

/// `json.dumps(value, cls=DjangoJSONEncoder)` for an ordered document.
fn dje_text(value: &Doc) -> Result<String, ViewError> {
    String::from_utf8(write_support::dje_bytes(value)?)
        .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `DjangoJSONEncoder`'s datetime: milliseconds, `Z` for UTC
/// (`django/core/serializers/json.py:93-99`).
fn dje_datetime(value: chrono::DateTime<Utc>) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &DjangoDateTime(value))
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let text = String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(text.trim_matches('"').to_owned())
}

/// `request.data.get(key)` for the two values this view reads before the
/// serializer. A non-mapping JSON body has no `.get` (a 500).
fn raw_body_value(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<PyValue>, ViewError> {
    match ctx.data()? {
        Data::Json(PyValue::Dict(object)) => Ok(object.get(key).cloned()),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: request.data is not a mapping".to_owned(),
        )),
        Data::Form(query) => Ok(query.get(key).map(|value| PyValue::Str(PyStr::new(value)))),
    }
}

/// Django's `UUIDField.get_prep_value` for a raw body value: anything that is
/// not a parsable uuid string raises `ValidationError`, which the family
/// `handle_exception` renders as `{"error": "Please provide valid detail"}`.
fn raw_uuid(value: &PyValue) -> Result<Uuid, ViewError> {
    let PyValue::Str(text) = value else {
        return Err(ViewError::Validation);
    };
    text.as_str()
        .ok_or(ViewError::Validation)
        .and_then(|text| Uuid::parse_str(text).map_err(|_| ViewError::Validation))
}

/// `Module.objects.filter(pk__in=module_ids, …)`: each element goes through the
/// uuid field's prep, and a non-list is iterated the way Python does (`str`
/// per character, `dict` by key) — a scalar is a `TypeError`.
fn raw_uuid_list(value: &PyValue) -> Result<Vec<Uuid>, ViewError> {
    match value {
        PyValue::List(items) => items.iter().map(raw_uuid).collect(),
        PyValue::Str(text) => text
            .as_str()
            .unwrap_or_default()
            .chars()
            .map(|_| Err(ViewError::Validation))
            .collect(),
        PyValue::Dict(object) => object
            .iter()
            .map(|(key, _)| {
                key.as_str()
                    .ok_or(ViewError::Validation)
                    .and_then(|key| Uuid::parse_str(key).map_err(|_| ViewError::Validation))
            })
            .collect(),
        PyValue::Null => Ok(Vec::new()),
        other => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not iterable",
            python_type(other)
        ))),
    }
}

/// `len(set(module_ids))` over the raw values (not their uuid projections).
fn distinct_raw(value: &PyValue) -> Result<usize, ViewError> {
    match value {
        PyValue::List(items) => {
            let mut seen = std::collections::HashSet::with_capacity(items.len());
            for item in items {
                seen.insert(write_support::python_str(item));
            }
            Ok(seen.len())
        }
        PyValue::Str(text) => Ok(text.as_str().unwrap_or_default().chars().count()),
        PyValue::Dict(object) => Ok(object.len()),
        PyValue::Null => Ok(0),
        other => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not iterable",
            python_type(other)
        ))),
    }
}

fn python_type(value: &PyValue) -> &'static str {
    match value {
        PyValue::Null => "NoneType",
        PyValue::Bool(_) => "bool",
        PyValue::Int(_) => "int",
        PyValue::Float(_) => "float",
        PyValue::Str(_) => "str",
        PyValue::List(_) => "list",
        PyValue::Dict(_) => "dict",
    }
}

/// `self.initial_data.get(name)` for the two arrays the serializer's
/// `to_representation` echoes back raw.
fn assignee_raw(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<PyValue>, ViewError> {
    raw_body_value(ctx, key)
}

/// `value if value else []` as the JSON array `to_representation` writes.
fn raw_id_array(value: &Option<PyValue>) -> Doc {
    match value {
        Some(PyValue::List(items)) => Doc::Array(items.iter().map(pyjson_to_json).collect()),
        _ => Doc::Array(Vec::new()),
    }
}

async fn relation_ids(
    client: &deadpool_postgres::Client,
    sql: &str,
    issue_id: Uuid,
) -> Result<Vec<Uuid>, ViewError> {
    let rows = client.query(sql, &[&issue_id]).await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

fn parse_json(value: &JsonText) -> Result<Doc, ViewError> {
    let parsed = plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(pyjson_to_json(&parsed))
}

fn pyjson_to_json(value: &PyValue) -> Doc {
    match value {
        PyValue::Null => Doc::Null,
        PyValue::Bool(flag) => Doc::Bool(*flag),
        PyValue::Int(number) => number.as_i64().map_or_else(
            || Doc::Float(number.to_string().parse().unwrap_or_default()),
            Doc::Int,
        ),
        PyValue::Float(number) => Doc::Float(*number),
        PyValue::Str(text) => Doc::string(text.as_str().unwrap_or_default()),
        PyValue::List(items) => Doc::Array(items.iter().map(pyjson_to_json).collect()),
        PyValue::Dict(object) => Doc::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().unwrap_or_default().to_owned(),
                        pyjson_to_json(value),
                    )
                })
                .collect(),
        ),
    }
}

/// `IssueCreateSerializer(issue).data` (`plane/app/serializers/issue.py:126-132`)
/// for the 201 body.
///
/// 36 keys: the 34 readable fields in `self.fields` order, then the two
/// write-only arrays `to_representation` appends from `initial_data`.
#[allow(clippy::too_many_arguments)]
fn issue_create_representation(
    issue: &Issue,
    timezone: chrono_tz::Tz,
    assignees: &[Uuid],
    labels: &[Uuid],
    description_json: Doc,
    assignee_ids: Doc,
    label_ids: Doc,
) -> Doc {
    let zoned = |value: chrono::DateTime<Utc>| {
        Doc::DateTimeZoned(value.with_timezone(&timezone).fixed_offset())
    };
    let zoned_opt = |value: Option<chrono::DateTime<Utc>>| value.map_or(Doc::Null, zoned);
    Doc::Object(
        vec![
            ("id".to_owned(), Doc::uuid(issue.id)),
            ("state_id".to_owned(), Doc::opt_uuid(issue.state_id)),
            ("parent_id".to_owned(), Doc::opt_uuid(issue.parent_id)),
            ("project_id".to_owned(), Doc::uuid(issue.project_id)),
            ("workspace_id".to_owned(), Doc::uuid(issue.workspace_id)),
            ("created_at".to_owned(), zoned(issue.created_at)),
            ("updated_at".to_owned(), zoned(issue.updated_at)),
            ("deleted_at".to_owned(), zoned_opt(issue.deleted_at)),
            (
                "point".to_owned(),
                issue
                    .point
                    .map_or(Doc::Null, |value| Doc::Int(i64::from(value))),
            ),
            ("name".to_owned(), Doc::string(issue.name.as_str())),
            ("description_json".to_owned(), description_json),
            (
                "description_html".to_owned(),
                Doc::string(issue.description_html.as_str()),
            ),
            (
                "description_stripped".to_owned(),
                Doc::opt_string(issue.description_stripped.as_deref()),
            ),
            // `ModelField` of a `BinaryField` always reads back NULL here: the
            // serializer never writes the column.
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
                Doc::opt_string(issue.external_source.as_deref()),
            ),
            (
                "external_id".to_owned(),
                Doc::opt_string(issue.external_id.as_deref()),
            ),
            ("created_by".to_owned(), Doc::opt_uuid(issue.created_by_id)),
            ("updated_by".to_owned(), Doc::opt_uuid(issue.updated_by_id)),
            ("project".to_owned(), Doc::uuid(issue.project_id)),
            ("workspace".to_owned(), Doc::uuid(issue.workspace_id)),
            ("parent".to_owned(), Doc::opt_uuid(issue.parent_id)),
            ("state".to_owned(), Doc::opt_uuid(issue.state_id)),
            (
                "estimate_point".to_owned(),
                Doc::opt_uuid(issue.estimate_point_id),
            ),
            ("type".to_owned(), Doc::opt_uuid(issue.type_id)),
            (
                "assignees".to_owned(),
                Doc::Array(assignees.iter().copied().map(Doc::uuid).collect()),
            ),
            (
                "labels".to_owned(),
                Doc::Array(labels.iter().copied().map(Doc::uuid).collect()),
            ),
            ("assignee_ids".to_owned(), assignee_ids),
            ("label_ids".to_owned(), label_ids),
        ]
        .into_iter()
        .collect(),
    )
}

// `can_write_project(user, slug, project_id)` (`plane/app/views/workspace/
// draft.py:48-64`): an active ADMIN/MEMBER project role, or a workspace ADMIN
// who is an active member of the project at any role.
async fn can_write_project(
    client: &deadpool_postgres::Client,
    user: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<bool, ViewError> {
    let role: Option<i16> = client
        .query_opt(queries::PROJECT_ROLE, &[&user, &workspace_id, &project_id])
        .await?
        .map(|row| row.get(0));
    let Some(role) = role else {
        return Ok(false);
    };
    if role == 20 || role == 15 {
        return Ok(true);
    }
    Ok(client
        .query_opt(queries::WORKSPACE_ADMIN, &[&user, &workspace_id])
        .await?
        .is_some())
}

fn validate_description(raw: Option<&str>) -> Result<String, ViewResponse> {
    let raw = raw.unwrap_or("<p></p>");
    match description::validate_html_content(raw) {
        description::HtmlValidation::Invalid => Err(write::field_string_error(
            "error",
            "html content is not valid",
        )),
        description::HtmlValidation::Unchanged => Ok(raw.to_owned()),
        description::HtmlValidation::Sanitized(html) => Ok(html),
    }
}

fn value_uuid(values: &write::Validated, name: &str) -> Option<Uuid> {
    values.uuid(name)
}

async fn filter_assignees(
    client: &deadpool_postgres::Client,
    project_id: Option<Uuid>,
    ids: Option<&[Uuid]>,
) -> Result<Vec<Uuid>, ViewError> {
    let Some(project_id) = project_id else {
        return Ok(Vec::new());
    };
    let Some(ids) = ids else {
        return Ok(Vec::new());
    };
    let rows = client
        .query(queries::ASSIGNEE_IDS, &[&project_id, &ids])
        .await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

async fn filter_labels(
    client: &deadpool_postgres::Client,
    project_id: Option<Uuid>,
    ids: Option<&[Uuid]>,
) -> Result<Vec<Uuid>, ViewError> {
    let Some(project_id) = project_id else {
        return Ok(Vec::new());
    };
    let Some(ids) = ids else {
        return Ok(Vec::new());
    };
    let rows = client
        .query(queries::LABEL_IDS, &[&project_id, &ids])
        .await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

async fn insert_draft(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
) -> Result<(), ViewError> {
    client
        .execute(
            queries::DRAFT_INSERT,
            &[
                &model.created_at,
                &model.updated_at,
                &model.created_by_id,
                &model.updated_by_id,
                &model.deleted_at,
                &model.id,
                &model.workspace_id,
                &model.project_id,
                &model.parent_id,
                &model.state_id,
                &model.estimate_point_id,
                &model.name,
                &model.description_json,
                &model.description_html,
                &model.description_stripped,
                &model.description_binary,
                &model.priority,
                &model.start_date,
                &model.target_date,
                &model.sort_order,
                &model.completed_at,
                &model.external_source,
                &model.external_id,
                &model.type_id,
            ],
        )
        .await?;
    Ok(())
}

async fn update_draft(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
) -> Result<(), ViewError> {
    client
        .execute(
            queries::DRAFT_UPDATE,
            &[
                &model.created_at,
                &model.updated_at,
                &model.created_by_id,
                &model.updated_by_id,
                &model.deleted_at,
                &model.workspace_id,
                &model.project_id,
                &model.parent_id,
                &model.state_id,
                &model.estimate_point_id,
                &model.name,
                &model.description_json,
                &model.description_html,
                &model.description_stripped,
                &model.description_binary,
                &model.priority,
                &model.start_date,
                &model.target_date,
                &model.sort_order,
                &model.completed_at,
                &model.external_source,
                &model.external_id,
                &model.type_id,
                &model.id,
            ],
        )
        .await?;
    Ok(())
}

async fn insert_m2m(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
    assignees: &[Uuid],
    labels: &[Uuid],
    cycle: Option<Uuid>,
    modules: &[Uuid],
    now: chrono::DateTime<Utc>,
) -> Result<(), ViewError> {
    for assignee in assignees {
        client
            .execute(
                queries::INSERT_ASSIGNEE,
                &[
                    &now,
                    &now,
                    &Uuid::new_v4(),
                    assignee,
                    &model.created_by_id,
                    &model.id,
                    &model.project_id,
                    &model.updated_by_id,
                    &model.workspace_id,
                ],
            )
            .await?;
    }
    for label in labels {
        client
            .execute(
                queries::INSERT_LABEL,
                &[
                    &now,
                    &now,
                    &Uuid::new_v4(),
                    &model.created_by_id,
                    &model.id,
                    label,
                    &model.project_id,
                    &model.updated_by_id,
                    &model.workspace_id,
                ],
            )
            .await?;
    }
    if let Some(cycle) = cycle {
        insert_cycle(client, model, cycle, now).await?;
    }
    insert_modules(client, model, modules, now).await?;
    Ok(())
}

async fn insert_cycle(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
    cycle: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<(), ViewError> {
    client
        .execute(
            queries::INSERT_CYCLE,
            &[
                &now,
                &now,
                &Uuid::new_v4(),
                &model.created_by_id,
                &cycle,
                &model.id,
                &model.project_id,
                &model.updated_by_id,
                &model.workspace_id,
            ],
        )
        .await?;
    Ok(())
}

async fn insert_modules(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
    modules: &[Uuid],
    now: chrono::DateTime<Utc>,
) -> Result<(), ViewError> {
    for module in modules {
        client
            .execute(
                queries::INSERT_MODULE,
                &[
                    &now,
                    &now,
                    &Uuid::new_v4(),
                    &model.created_by_id,
                    &model.id,
                    module,
                    &model.project_id,
                    &model.updated_by_id,
                    &model.workspace_id,
                ],
            )
            .await?;
    }
    Ok(())
}

async fn replace_assignees(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
    assignees: &[Uuid],
    now: chrono::DateTime<Utc>,
) -> Result<(), ViewError> {
    client
        .execute(queries::SOFT_DELETE_ASSIGNEES, &[&now, &model.id])
        .await?;
    // `updated_by_id` is the pre-update value (Django's `instance` before
    // `super().update()`), i.e. the old `updated_by_id`.
    let created_by = model.created_by_id;
    let updated_by = model.updated_by_id;
    for assignee in assignees {
        client
            .execute(
                queries::INSERT_ASSIGNEE,
                &[
                    &now,
                    &now,
                    &Uuid::new_v4(),
                    assignee,
                    &created_by,
                    &model.id,
                    &model.project_id,
                    &updated_by,
                    &model.workspace_id,
                ],
            )
            .await?;
    }
    Ok(())
}

async fn replace_labels(
    client: &deadpool_postgres::Client,
    model: &DraftIssue,
    labels: &[Uuid],
    now: chrono::DateTime<Utc>,
) -> Result<(), ViewError> {
    client
        .execute(queries::SOFT_DELETE_LABELS, &[&now, &model.id])
        .await?;
    for label in labels {
        client
            .execute(
                queries::INSERT_LABEL,
                &[
                    &now,
                    &now,
                    &Uuid::new_v4(),
                    &model.created_by_id,
                    &model.id,
                    label,
                    &model.project_id,
                    &model.updated_by_id,
                    &model.workspace_id,
                ],
            )
            .await?;
    }
    Ok(())
}

fn non_field(message: &str) -> ViewResponse {
    write::field_string_error("non_field_errors", message)
}

fn data_string(ctx: &ViewCtx<'_>, key: &str) -> Option<String> {
    ctx.data()
        .ok()
        .and_then(|data| data.get(key).map(str::to_owned))
}

fn data_has(ctx: &ViewCtx<'_>, key: &str) -> bool {
    ctx.data()
        .ok()
        .and_then(|data| data.json())
        .and_then(|value| match value {
            Json::Dict(dict) => Some(dict.contains_key(key)),
            _ => None,
        })
        .unwrap_or(false)
}

fn data_uuid(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<Uuid>, ViewError> {
    let Some(value) = ctx.data()?.json().and_then(|value| match value {
        Json::Dict(dict) => dict.get(key),
        _ => None,
    }) else {
        return Ok(None);
    };
    match value {
        Json::Null => Ok(None),
        Json::Str(text) => text
            .as_str()
            .ok_or(ViewError::Validation)
            .and_then(|text| Uuid::parse_str(text).map_err(|_| ViewError::Validation))
            .map(Some),
        _ => Err(ViewError::Validation),
    }
}

fn data_uuid_list(ctx: &ViewCtx<'_>, key: &str) -> Result<Vec<Uuid>, ViewError> {
    let Some(value) = ctx.data()?.json().and_then(|value| match value {
        Json::Dict(dict) => dict.get(key),
        _ => None,
    }) else {
        return Ok(Vec::new());
    };
    let Json::List(items) = value else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if let Json::Str(text) = item
            && let Some(text) = text.as_str()
            && let Ok(uuid) = Uuid::parse_str(text)
        {
            out.push(uuid);
        }
    }
    Ok(out)
}

#[derive(serde::Serialize)]
struct DraftResponse {
    id: Uuid,
    name: Option<String>,
    state_id: Option<Uuid>,
    sort_order: f64,
    completed_at: Option<plane_serializers::fields::DrfDateTime<chrono_tz::Tz>>,
    estimate_point: Option<Uuid>,
    priority: String,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    project_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    cycle_id: Option<Uuid>,
    module_ids: Vec<Uuid>,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    created_at: plane_serializers::fields::DrfDateTime<chrono_tz::Tz>,
    updated_at: plane_serializers::fields::DrfDateTime<chrono_tz::Tz>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    type_id: Option<Uuid>,
    description_html: String,
}

async fn response_body(
    client: &deadpool_postgres::Client,
    id: Uuid,
    timezone: &chrono_tz::Tz,
) -> Result<Vec<u8>, ViewError> {
    let row = client
        .query_opt(queries::CREATE_RESPONSE, &[&id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    let body = DraftResponse {
        id: row.get(0),
        name: row.get(1),
        state_id: row.get(2),
        sort_order: row.get(3),
        completed_at: plane_serializers::fields::drf_datetime_opt(row.get(4), timezone),
        estimate_point: row.get(5),
        priority: row.get(6),
        start_date: row.get(7),
        target_date: row.get(8),
        project_id: row.get(9),
        parent_id: row.get(10),
        cycle_id: row.get(11),
        module_ids: row.get(12),
        label_ids: row.get(13),
        assignee_ids: row.get(14),
        created_at: plane_serializers::fields::drf_datetime(row.get(15), timezone),
        updated_at: plane_serializers::fields::drf_datetime(row.get(16), timezone),
        created_by: row.get(17),
        updated_by: row.get(18),
        type_id: row.get(19),
        description_html: row.get(20),
    };
    Ok(plane_serde::to_vec(plane_serde::Dialect::Drf, &body)?)
}
