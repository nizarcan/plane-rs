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
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks::draft_issue;
use plane_db::models::draft::DraftIssue;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::issues::write_draft_queries as queries;
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
