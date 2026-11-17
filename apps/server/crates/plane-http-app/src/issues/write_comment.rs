// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Set B of the work-items plan: comments and reactions.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/issues/{issue_id}/comments/` | `IssueCommentViewSet.create` | `comment.py:104-170` |
//! | `PATCH …/issues/{issue_id}/comments/{pk}/` | `partial_update` | `comment.py:172-220` |
//! | `DELETE …/issues/{issue_id}/comments/{pk}/` | `destroy` | `comment.py:222-245` |
//! | `POST …/issues/{issue_id}/reactions/` | `IssueReactionViewSet.create` | `reaction.py:56-75` |
//! | `DELETE …/issues/{issue_id}/reactions/{code}/` | `destroy` | `reaction.py:77-98` |
//! | `POST …/comments/{comment_id}/reactions/` | `CommentReactionViewSet.create` | `comment.py:247-296` |
//! | `DELETE …/comments/{comment_id}/reactions/{code}/` | `destroy` | `comment.py:298-333` |
//!
//! ## Comment create
//!
//! 1. `Project.objects.get(pk=project_id)` (`comment.py:123`) — no slug filter;
//! 2. `Issue.objects.filter(pk=issue_id, workspace__slug=slug,
//!    project_id=project_id).first()` → 404 `{"error": "Issue not found"}`;
//! 3. the restricted-guest rule: an active project member with role 5, on a
//!    project with `guest_view_all_features=False`, commenting on an issue they
//!    did not create → 400;
//! 4. `request.data` filtered to `comment_html`, `comment_json`, `access` and
//!    validated by `IssueCommentSerializer`;
//! 5. `IssueComment.save()`: H14 strips HTML, then **one transaction** writes
//!    the comment, creates the `Description` (H28) and column-updates
//!    `description_id`;
//! 6. `issue_activity` (`comment.activity.created`), then `model_activity`.
//!
//! ## Comment PATCH / DELETE
//!
//! `IssueComment.objects.get(...)` (family 404 on a miss), the filtered body,
//! `requested_data = json.dumps(data)` and `current_instance =
//! json.dumps(IssueCommentSerializer(comment).data)` — both before the save.
//! PATCH sets `edited_at` only when the raw `comment_html` differs from the
//! stored one; the description sync updates only the changed tracked fields.
//! DELETE soft-deletes (H03 + cascade task) and then publishes the activity, so
//! the cascade task precedes it on the queue.
//!
//! ## Reactions
//!
//! Duplicate handling differs by owner: an issue-reaction duplicate is an
//! uncaught `IntegrityError` (family 400 "The payload is not valid"), a
//! comment-reaction duplicate is caught and answered with "Reaction already
//! exists for the user". Both deletes publish the activity **before** the soft
//! delete (so before the cascade task) and only the reaction's own actor can
//! delete their row. Comment reactions run the restricted-guest check first;
//! issue reactions do not.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::audit::Audit;
use plane_db::models::issue::{IssueComment, IssueReaction};
use plane_db::{JsonText, hooks::issue_comment as comment_hook};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall, ModelActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::RenderCtx;
use plane_serializers::generated::app_issue::IssueReactionSerializer;
use uuid::Uuid;

use crate::common::client;
use crate::issues::sub_resources::sub_common::{
    self as shared, CommentEntry, CommentReactionRow, CommentRow, ProjectLiteRow, UserLiteRow,
    WorkspaceLiteRow,
};
use crate::issues::sub_resources::sub_queries;
use crate::issues::write_comment_queries as queries;
use crate::write::description::{self, HtmlValidation};
use crate::write::{self, CharOptions, Serializer};

/// `COMMENT_WRITABLE_FIELDS` (`comment.py:25`): everything else in the body is
/// ignored by the view before the serializer sees it.
const COMMENT_WRITABLE_FIELDS: [&str; 3] = ["comment_html", "comment_json", "access"];

/// `IssueComment.access`'s `choices` (`plane/db/models/issue.py:467-471`).
const ACCESS_CHOICES: &[&str] = &["INTERNAL", "EXTERNAL"];

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

fn kwarg_uuid(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    let value = ctx.kwarg(name).ok_or(ViewError::Key)?;
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
}

fn origin(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

fn epoch() -> i64 {
    Utc::now().timestamp()
}

/// Django's `IntegrityError` family (SQLSTATE class 23) → the family 400.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// A keyed error body whose value is a bare string (`validate()`'s
/// `ValidationError({"comment_html": "…"})`), unlike a field error's list.
fn single_string_error(field: &str, message: &str) -> ViewResponse {
    let body = plane_query::json::Json::Object(vec![(
        field.to_owned(),
        plane_query::json::Json::string(message),
    )])
    .to_drf();
    ViewResponse::raw(400, APPLICATION_JSON, body)
}

/// `{"error": …}`.
fn error_body(status: u16, message: &str) -> ViewResponse {
    write::error_body(status, message)
}

/// `request.data` as a Python value, for `json.dumps(request.data)`.
fn request_data_value(ctx: &ViewCtx<'_>) -> Result<Json, ViewError> {
    match ctx.data()? {
        Data::Json(value) => Ok(value.clone()),
        Data::Form(query) => {
            let mut map = PyDict::new();
            for (key, value) in query.iter() {
                map.insert(PyStr::new(key), Json::Str(PyStr::new(value)));
            }
            Ok(Json::Dict(map))
        }
    }
}

/// `{key: request.data[key] for key in COMMENT_WRITABLE_FIELDS if key in
/// request.data}`. A non-object body is a Python `TypeError` (500) here,
/// before the serializer.
fn filtered_body(ctx: &ViewCtx<'_>) -> Result<PyDict, ViewError> {
    match ctx.data()? {
        Data::Json(Json::Dict(map)) => {
            let mut out = PyDict::new();
            for key in COMMENT_WRITABLE_FIELDS {
                if let Some(value) = map.get(key) {
                    out.insert(PyStr::new(key), value.clone());
                }
            }
            Ok(out)
        }
        Data::Form(query) => {
            let mut out = PyDict::new();
            for key in COMMENT_WRITABLE_FIELDS {
                if let Some(value) = query.get(key) {
                    out.insert(PyStr::new(key), Json::Str(PyStr::new(value)));
                }
            }
            Ok(out)
        }
        Data::Json(_) => Err(ViewError::Unknown(
            "TypeError: a non-object request body has no keys".to_string(),
        )),
    }
}

/// `json.dumps(value, cls=DjangoJSONEncoder)`.
fn django_json(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `json.dumps(serializer.data, cls=DjangoJSONEncoder)` for a comment.
fn comment_json_string(
    row: &CommentRow,
    reactions: &[CommentReactionRow],
    timezone: Tz,
) -> Result<String, ViewError> {
    django_json(&comment_value(row, reactions, timezone))
}

// ---------------------------------------------------------------------------
// Comment validation
// ---------------------------------------------------------------------------

#[derive(Default, Debug)]
struct CommentInput {
    html: Option<String>,
    json: Option<JsonText>,
    access: Option<String>,
}

/// `IssueCommentSerializer(data=data)` + `validate()` for the three
/// client-writable fields.
fn validate_comment(ctx: &ViewCtx<'_>) -> Result<Result<CommentInput, ViewResponse>, ViewError> {
    let mut serializer = Serializer::new(ctx, false)?;
    // `IssueCommentSerializer.FIELDS` order: `comment_json` precedes
    // `comment_html` precedes `access`, so error keys are in that order.
    let json = serializer.json_field("comment_json", false);
    let html = serializer.char_field(
        "comment_html",
        CharOptions {
            allow_null: false,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    let access = serializer.choice_field("access", false, false, ACCESS_CHOICES);
    if let Err(errors) = serializer.finish() {
        return Ok(Err(errors.into_response()));
    }

    // `validate()`: a truthy `comment_html` is sanitised; over the 10 MB limit
    // is the only failure (`validate_html_content`).
    let html = match html {
        Some(value) if !value.is_empty() => match description::validate_html_content(&value) {
            HtmlValidation::Unchanged => Some(value),
            HtmlValidation::Sanitized(clean) => Some(clean),
            HtmlValidation::Invalid => {
                return Ok(Err(single_string_error(
                    "comment_html",
                    "HTML content is not valid",
                )));
            }
        },
        other => other,
    };
    Ok(Ok(CommentInput { html, json, access }))
}

// ---------------------------------------------------------------------------
// Comment handlers
// ---------------------------------------------------------------------------

/// `IssueCommentViewSet.create` (`comment.py:104-170`).
pub fn comment_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Project.objects.get(pk=project_id)`.
        let statement = queries::PROJECT_FOR_COMMENT.prepare(client).await?;
        let Some(project) = client.query_opt(&statement, &[&project_id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let guest_view_all_features: bool = project.get(2);

        // The URL issue must belong to the URL workspace + project.
        let statement = queries::ISSUE_FOR_COMMENT.prepare(client).await?;
        let Some(issue) = client
            .query_opt(&statement, &[&issue_id, &slug, &project_id])
            .await?
        else {
            return Ok(error_body(404, "Issue not found"));
        };
        let issue_workspace: Uuid = issue.get(1);
        let issue_creator: Option<Uuid> = issue.get(2);

        // Restricted guest: project role 5, the feature off, not the issue's
        // creator.
        let statement = queries::PROJECT_GUEST_MEMBERSHIP.prepare(client).await?;
        let is_guest = client
            .query_opt(&statement, &[&slug, &project_id, &user])
            .await?
            .is_some();
        if is_guest && !guest_view_all_features && issue_creator != Some(user) {
            return Ok(error_body(
                400,
                "You are not allowed to comment on the issue",
            ));
        }

        // The view's filtered body (parsed after the checks, as in Python).
        let data = filtered_body(ctx)?;
        let input = match validate_comment(ctx)? {
            Ok(input) => input,
            Err(response) => return Ok(response),
        };

        let now = write::now();
        let mut comment = IssueComment {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id: issue_workspace,
            comment_stripped: String::new(),
            comment_json: input.json.unwrap_or_else(|| JsonText::new("{}")),
            comment_html: input.html.unwrap_or_else(|| "<p></p>".to_string()),
            description_id: None,
            attachments: Vec::new(),
            issue_id,
            actor_id: Some(user),
            access: input.access.unwrap_or_else(|| "INTERNAL".to_string()),
            external_source: None,
            external_id: None,
            edited_at: None,
            parent_id: None,
        };
        comment_hook::before_save(&mut comment);

        // `IssueComment.save()`'s `transaction.atomic()` block. The request's
        // connection is borrowed immutably by `common::client`, so the
        // boundaries are the same `BEGIN`/`COMMIT` the ORM issues.
        client
            .batch_execute("BEGIN")
            .await
            .map_err(ViewError::from)?;
        if let Err(error) = insert_comment(client, &mut comment, user).await {
            let _ = client.batch_execute("ROLLBACK").await;
            return Err(error);
        }
        client
            .batch_execute("COMMIT")
            .await
            .map_err(ViewError::from)?;
        // The second, column-only save is an *update* for `BaseModel.save`, so
        // it sets `updated_by` on the in-memory instance (the column itself is
        // not in `update_fields`, so the row keeps NULL). `serializer.data`
        // renders that in-memory value.
        comment.updated_by_id = Some(user);

        // The response/activity row, with the in-memory JSON column (the
        // request's key order, not jsonb's) and in-memory audit value.
        let (mut row, reactions) =
            load_comment(ctx, client, slug, project_id, issue_id, comment.id).await?;
        row.comment.comment_json = comment.comment_json.clone();
        row.comment.updated_by_id = comment.updated_by_id;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let current_site = origin(ctx)?;

        // `issue_activity.delay(comment.activity.created, json.dumps(serializer.data), …)`.
        let requested_data = comment_json_string(&row, &reactions, timezone)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "comment.activity.created",
                TaskArg::json_text(requested_data),
                user.to_string(),
                TaskArg::str(ctx.kwarg("issue_id").unwrap_or_default()),
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::Null,
                epoch(),
            ),
            current_site.clone(),
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        // `model_activity.delay(model_name="issue_comment", model_id=str(id), …)`.
        let model = tasks::model_activity(ModelActivityCall {
            model_name: "issue_comment".to_string(),
            model_id: comment.id.to_string(),
            requested_data: TaskArg::Json(Json::Dict(data)),
            current_instance: None,
            actor_id: user,
            slug: slug.to_string(),
            origin: current_site,
        });
        crate::common::jobs::dispatch_task(
            ctx,
            model.task(),
            &model,
            plane_jobs::queue::Job::new(model.task()),
        )
        .await?;

        comment_response(ctx, row, &reactions, 201)
    })
}

/// `IssueCommentViewSet.partial_update` (`comment.py:172-220`).
pub fn comment_partial_update<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let (mut row, reactions) =
            load_comment(ctx, client, slug, project_id, issue_id, pk).await?;

        let data = filtered_body(ctx)?;
        let requested_data = django_json(&Json::Dict(data.clone()))?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let current_instance = comment_json_string(&row, &reactions, timezone)?;

        let input = match validate_comment(ctx)? {
            Ok(input) => input,
            Err(response) => return Ok(response),
        };

        let old = row.comment.clone();
        // Python compares the **raw** body value to the stored html
        // (`comment.py:190-193`), not the sanitised one.
        let edited_at = data
            .get("comment_html")
            .filter(|value| raw_differs_from(value, &old.comment_html))
            .map(|_| write::now());
        if let Some(html) = input.html {
            row.comment.comment_html = html;
        }
        if let Some(json) = input.json {
            row.comment.comment_json = json;
        }
        if let Some(access) = input.access {
            row.comment.access = access;
        }
        if let Some(edited_at) = edited_at {
            row.comment.edited_at = Some(edited_at);
        }
        comment_hook::before_save(&mut row.comment);
        row.comment.updated_at = write::now();
        row.comment.updated_by_id = Audit::on_update(Some(user));

        let changes = comment_hook::description_changes(&old, &row.comment);
        save_comment(client, &row.comment, &changes).await?;

        let current_site = origin(ctx)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "comment.activity.updated",
                TaskArg::json_text(requested_data),
                user.to_string(),
                TaskArg::str(ctx.kwarg("issue_id").unwrap_or_default()),
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::json_text(current_instance.clone()),
                epoch(),
            ),
            current_site.clone(),
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        let model = tasks::model_activity(ModelActivityCall {
            model_name: "issue_comment".to_string(),
            model_id: ctx.kwarg("pk").unwrap_or_default().to_string(),
            requested_data: TaskArg::Json(Json::Dict(data)),
            current_instance: Some(current_instance),
            actor_id: user,
            slug: slug.to_string(),
            origin: current_site,
        });
        crate::common::jobs::dispatch_task(
            ctx,
            model.task(),
            &model,
            plane_jobs::queue::Job::new(model.task()),
        )
        .await?;

        comment_response(ctx, row, &reactions, 200)
    })
}

/// `IssueCommentViewSet.destroy` (`comment.py:222-245`).
pub fn comment_destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let (row, reactions) = load_comment(ctx, client, slug, project_id, issue_id, pk).await?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let current_instance = comment_json_string(&row, &reactions, timezone)?;

        // `issue_comment.delete()`: H03 then the cascade task.
        let now = write::now();
        write::soft_delete(
            client,
            "issue_comments",
            "id",
            pk,
            Audit::on_update(Some(user)),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuecomment", pk).await?;

        let current_site = origin(ctx)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "comment.activity.deleted",
                TaskArg::json_text(django_json(&Json::Dict({
                    let mut map = PyDict::new();
                    map.insert(
                        PyStr::new("comment_id"),
                        Json::Str(PyStr::new(&pk.to_string())),
                    );
                    map
                }))?),
                user.to_string(),
                TaskArg::str(ctx.kwarg("issue_id").unwrap_or_default()),
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::json_text(current_instance),
                epoch(),
            ),
            current_site,
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        Ok(ViewResponse::empty(204))
    })
}

/// The `IssueComment.save()` UPDATE plus the description sync, in one
/// transaction (`plane/db/models/issue.py:503-531`).
async fn save_comment(
    client: &deadpool_postgres::Client,
    comment: &IssueComment,
    changes: &comment_hook::DescriptionChanges,
) -> Result<(), ViewError> {
    client
        .batch_execute("BEGIN")
        .await
        .map_err(ViewError::from)?;
    let result = save_comment_rows(client, comment, changes).await;
    match result {
        Ok(()) => {
            client
                .batch_execute("COMMIT")
                .await
                .map_err(ViewError::from)?;
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            return Err(error);
        }
    }
    Ok(())
}

/// The two statements of [`save_comment`], inside the caller's transaction.
async fn save_comment_rows(
    client: &deadpool_postgres::Client,
    comment: &IssueComment,
    changes: &comment_hook::DescriptionChanges,
) -> Result<(), ViewError> {
    let statement = queries::COMMENT_UPDATE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &comment.created_at,
                &comment.updated_at,
                &comment.created_by_id,
                &comment.updated_by_id,
                &comment.deleted_at,
                &comment.id,
                &comment.project_id,
                &comment.workspace_id,
                &comment.comment_stripped,
                &comment.comment_json,
                &comment.comment_html,
                &comment.description_id,
                &comment.attachments,
                &comment.issue_id,
                &comment.actor_id,
                &comment.access,
                &comment.external_source,
                &comment.external_id,
                &comment.edited_at,
                &comment.parent_id,
                &comment.id,
            ],
        )
        .await
        .map_err(integrity)?;

    if changes.any()
        && let Some(description_id) = comment.description_id
    {
        let mut sql = String::from("UPDATE descriptions SET ");
        let mut sets: Vec<&str> = Vec::new();
        if changes.html {
            sets.push("description_html = $1");
        }
        if changes.stripped {
            sets.push(if changes.html {
                "description_stripped = $2"
            } else {
                "description_stripped = $1"
            });
        }
        if changes.json {
            sets.push(if changes.html && changes.stripped {
                "description_json = $3"
            } else if changes.html || changes.stripped {
                "description_json = $2"
            } else {
                "description_json = $1"
            });
        }
        sql.push_str(&sets.join(", "));
        let next = sets.len() + 1;
        sql.push_str(&format!(
            ", updated_by_id = ${next}, updated_at = ${} WHERE id = ${}",
            next + 1,
            next + 2
        ));

        let text_json = comment.comment_json.clone();
        let mut args: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
        if changes.html {
            args.push(&comment.comment_html);
        }
        if changes.stripped {
            args.push(&comment.comment_stripped);
        }
        if changes.json {
            args.push(&text_json);
        }
        let updated_by = comment.updated_by_id;
        let updated_at = comment.updated_at;
        args.push(&updated_by);
        args.push(&updated_at);
        args.push(&description_id);
        let statement = client
            .prepare(sql.as_str())
            .await
            .map_err(ViewError::from)?;
        client.execute(&statement, &args).await.map_err(integrity)?;
    }
    Ok(())
}

/// The three statements of the create's atomic block:
/// `IssueComment.save()` → first INSERT, `Description.objects.create`, then the
/// column-only `update_fields=["description_id"]` save.
async fn insert_comment(
    client: &deadpool_postgres::Client,
    comment: &mut IssueComment,
    user: Uuid,
) -> Result<(), ViewError> {
    let statement = queries::COMMENT_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &comment.created_at,
                &comment.updated_at,
                &comment.created_by_id,
                &comment.updated_by_id,
                &comment.id,
                &comment.project_id,
                &comment.workspace_id,
                &comment.comment_stripped,
                &comment.comment_json,
                &comment.comment_html,
                &comment.attachments,
                &comment.issue_id,
                &comment.actor_id,
                &comment.access,
            ],
        )
        .await
        .map_err(integrity)?;

    let description_id = Uuid::new_v4();
    let description_now = write::now();
    let statement = queries::DESCRIPTION_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &description_now,
                &description_now,
                &description_id,
                &comment.comment_json,
                &comment.comment_html,
                &comment_hook::description_stripped(&comment.comment_html),
                &Some(user),
                &comment.project_id,
                &comment.workspace_id,
            ],
        )
        .await
        .map_err(integrity)?;
    comment.description_id = Some(description_id);
    let statement = queries::COMMENT_SET_DESCRIPTION.prepare(client).await?;
    client
        .execute(&statement, &[&description_id, &comment.id])
        .await
        .map_err(integrity)?;
    Ok(())
}

/// `IssueComment.objects.get(workspace__slug=…, project_id=…, issue_id=…,
/// issue__project_id=…, pk=…)` plus the comment's reactions.
async fn load_comment(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
) -> Result<(CommentRow, Vec<CommentReactionRow>), ViewError> {
    let statement = queries::COMMENT_ROW_BY_ID.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&pk, &slug, &project_id, &issue_id])
        .await?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let row = CommentRow::from_row(&row, 0, false)?;
    let reactions = comment_reactions(ctx, client, &[pk]).await?;
    Ok((row, reactions))
}

async fn comment_reactions(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    comments: &[Uuid],
) -> Result<Vec<CommentReactionRow>, ViewError> {
    let _ = ctx;
    let statement = sub_queries::COMMENT_REACTIONS.prepare(client).await?;
    let rows = client.query(&statement, &[&comments]).await?;
    rows.iter()
        .map(|row| CommentReactionRow::from_row(row, 0).map_err(ViewError::from))
        .collect()
}

/// `IssueCommentSerializer(row).data` rendered with the request's timezone.
fn comment_response(
    ctx: &ViewCtx<'_>,
    row: CommentRow,
    reactions: &[CommentReactionRow],
    status: u16,
) -> Result<ViewResponse, ViewError> {
    let refs: Vec<&CommentReactionRow> = reactions.iter().collect();
    let entry = CommentEntry::new(&row, &refs);
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(&entry.serializer(), &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

// ---------------------------------------------------------------------------
// Issue reactions (routes 86/87)
// ---------------------------------------------------------------------------

/// `IssueReactionViewSet.create` (`reaction.py:56-75`).
pub fn issue_reaction_create<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let statement = queries::ISSUE_FOR_REACTION.prepare(client).await?;
        let Some(issue) = client
            .query_opt(&statement, &[&issue_id, &slug, &project_id])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = issue.get(1);

        let reaction = match validate_reaction(ctx, true).await? {
            Ok(reaction) => reaction,
            Err(response) => return Ok(response),
        };

        let now = write::now();
        let reaction_id = Uuid::new_v4();
        let statement = queries::ISSUE_REACTION_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &reaction_id,
                    &project_id,
                    &workspace_id,
                    &user,
                    &issue_id,
                    &reaction,
                ],
            )
            .await
            .map_err(integrity)?;

        let requested_data = request_data_value(ctx)?;
        let current_site = origin(ctx)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "issue_reaction.activity.created",
                TaskArg::json_text(django_json(&requested_data)?),
                user.to_string(),
                TaskArg::str(ctx.kwarg("issue_id").unwrap_or_default()),
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::Null,
                epoch(),
            ),
            current_site,
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        let statement = queries::ISSUE_REACTION_BY_ID.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&reaction_id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction = IssueReaction::from_row(&row, 0)?;
        let actor = UserLiteRow::from_row(&row, IssueReaction::COLUMN_COUNT)?;
        let actor_serializer = actor.as_ref().map(UserLiteRow::serializer);
        let serializer = IssueReactionSerializer {
            id: reaction.id,
            actor_detail: actor_serializer.as_ref(),
            created_at: reaction.created_at,
            updated_at: reaction.updated_at,
            deleted_at: reaction.deleted_at,
            reaction: &reaction.reaction,
            created_by: reaction.created_by_id,
            updated_by: reaction.updated_by_id,
            project: reaction.project_id,
            workspace: reaction.workspace_id,
            actor: reaction.actor_id,
            issue: reaction.issue_id,
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `IssueReactionViewSet.destroy` (`reaction.py:77-98`).
pub fn issue_reaction_destroy<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let reaction_code = ctx.kwarg("reaction_code").ok_or(ViewError::Key)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let statement = queries::ISSUE_REACTION_TARGET.prepare(client).await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&slug, &project_id, &issue_id, &reaction_code, &user],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction_id: Uuid = row.get(0);

        let current_instance = {
            let mut map = PyDict::new();
            map.insert(PyStr::new("reaction"), Json::Str(PyStr::new(reaction_code)));
            map.insert(
                PyStr::new("identifier"),
                Json::Str(PyStr::new(&reaction_id.to_string())),
            );
            django_json(&Json::Dict(map))?
        };
        let current_site = origin(ctx)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "issue_reaction.activity.deleted",
                TaskArg::Null,
                user.to_string(),
                TaskArg::str(ctx.kwarg("issue_id").unwrap_or_default()),
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::json_text(current_instance),
                epoch(),
            ),
            current_site,
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        // `issue_reaction.delete()`: H03 + cascade.
        write::soft_delete(
            client,
            "issue_reactions",
            "id",
            reaction_id,
            Audit::on_update(Some(user)),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuereaction", reaction_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Comment reactions (routes 88/89)
// ---------------------------------------------------------------------------

/// `CommentReactionViewSet.create` (`comment.py:247-296`).
pub fn comment_reaction_create<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let comment_id = kwarg_uuid(ctx, "comment_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        if comment_hidden(ctx, client, slug, project_id, comment_id, user).await? {
            return Ok(error_body(403, "You are not allowed to view this issue"));
        }

        // The URL comment must belong to the URL workspace + project.
        let statement = queries::COMMENT_FOR_REACTION.prepare(client).await?;
        let Some(comment) = client
            .query_opt(&statement, &[&comment_id, &slug, &project_id])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = comment.get(1);

        let reaction = match validate_reaction(ctx, false).await? {
            Ok(reaction) => reaction,
            Err(response) => return Ok(response),
        };

        let now = write::now();
        let reaction_id = Uuid::new_v4();
        let statement = queries::COMMENT_REACTION_INSERT.prepare(client).await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &reaction_id,
                    &project_id,
                    &workspace_id,
                    &user,
                    &comment_id,
                    &reaction,
                ],
            )
            .await
        {
            if error
                .as_db_error()
                .is_some_and(|db| db.code().code().starts_with("23"))
            {
                return Ok(error_body(400, "Reaction already exists for the user"));
            }
            return Err(ViewError::from(error));
        }

        let requested_data = request_data_value(ctx)?;
        let current_site = origin(ctx)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "comment_reaction.activity.created",
                TaskArg::json_text(django_json(&requested_data)?),
                user.to_string(),
                TaskArg::Null,
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::Null,
                epoch(),
            ),
            current_site,
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        let statement = queries::COMMENT_REACTION_BY_ID.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&reaction_id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction = CommentReactionRow::from_row(&row, 0)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&reaction.serializer(), &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `CommentReactionViewSet.destroy` (`comment.py:298-333`).
pub fn comment_reaction_destroy<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let comment_id = kwarg_uuid(ctx, "comment_id")?;
        let reaction_code = ctx.kwarg("reaction_code").ok_or(ViewError::Key)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        if comment_hidden(ctx, client, slug, project_id, comment_id, user).await? {
            return Ok(error_body(403, "You are not allowed to view this issue"));
        }

        let statement = queries::COMMENT_REACTION_TARGET.prepare(client).await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&slug, &project_id, &comment_id, &reaction_code, &user],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction_id: Uuid = row.get(0);

        let current_instance = {
            let mut map = PyDict::new();
            map.insert(PyStr::new("reaction"), Json::Str(PyStr::new(reaction_code)));
            map.insert(
                PyStr::new("identifier"),
                Json::Str(PyStr::new(&reaction_id.to_string())),
            );
            map.insert(
                PyStr::new("comment_id"),
                Json::Str(PyStr::new(&comment_id.to_string())),
            );
            django_json(&Json::Dict(map))?
        };
        let current_site = origin(ctx)?;
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "comment_reaction.activity.deleted",
                TaskArg::Null,
                user.to_string(),
                TaskArg::Null,
                TaskArg::str(ctx.kwarg("project_id").unwrap_or_default()),
                TaskArg::json_text(current_instance),
                epoch(),
            ),
            current_site,
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        write::soft_delete(
            client,
            "comment_reactions",
            "id",
            reaction_id,
            Audit::on_update(Some(user)),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "commentreaction", reaction_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `_comment_issue_hidden_from_guest` (`comment.py:226-246`).
async fn comment_hidden(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    comment_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = sub_queries::COMMENT_ISSUE.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&comment_id, &project_id, &slug])
        .await?
    else {
        return Ok(false);
    };
    let issue_id: Uuid = row.get(0);
    let _ = ctx;
    shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user).await
}

/// `IssueReactionSerializer`/`CommentReactionSerializer`'s writable fields.
///
/// `IssueReactionSerializer` also accepts `created_by`/`updated_by` (its
/// `read_only_fields` do not cover them); H01 overwrites both on create, but
/// an invalid pk still fails validation.
async fn validate_reaction(
    ctx: &ViewCtx<'_>,
    with_audit_fields: bool,
) -> Result<Result<String, ViewResponse>, ViewError> {
    let client = client(ctx)?;
    let mut serializer = Serializer::new(ctx, false)?;
    let reaction = serializer.char_field(
        "reaction",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: None,
        },
    );
    if serializer.missing_required("reaction") {
        serializer.push_error("reaction", write::Detail::required());
    }
    // `IssueReactionSerializer.FIELDS` visits `reaction`, then `created_by`,
    // then `updated_by`; `CommentReactionSerializer` has only `reaction`.
    if with_audit_fields {
        use crate::write::generated::issue_reaction_serializer as reaction_fields;
        serializer
            .primary_key_field(client, "created_by", true, &reaction_fields::PK_CREATED_BY)
            .await?;
        serializer
            .primary_key_field(client, "updated_by", true, &reaction_fields::PK_UPDATED_BY)
            .await?;
    }
    match serializer.finish() {
        Ok(_) => {}
        Err(errors) => return Ok(Err(errors.into_response())),
    }
    let Some(reaction) = reaction else {
        // Required and only absent on a non-JSON body shape, which
        // `Serializer::new` already reported.
        return Err(ViewError::Unknown("reaction is required".to_string()));
    };
    Ok(Ok(reaction))
}

// ---------------------------------------------------------------------------
// Serializer-data as Python JSON (for the Celery strings)
// ---------------------------------------------------------------------------

fn dict(entries: Vec<(&str, Json)>) -> Json {
    let mut map = PyDict::new();
    for (key, value) in entries {
        map.insert(PyStr::new(key), value);
    }
    Json::Dict(map)
}

fn opt_uuid(value: Option<Uuid>) -> Json {
    value.map_or(Json::Null, |value| {
        Json::Str(PyStr::new(&value.to_string()))
    })
}

fn opt_text(value: Option<&str>) -> Json {
    value.map_or(Json::Null, |value| Json::Str(PyStr::new(value)))
}

fn json_column(value: &JsonText) -> Json {
    plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8").unwrap_or(Json::Null)
}

fn json_column_opt(value: Option<&JsonText>) -> Json {
    value.map_or(Json::Null, json_column)
}

fn python_datetime(value: DateTime<Utc>, timezone: Tz) -> String {
    let local = value.with_timezone(&timezone);
    let mut text = if local.timestamp_subsec_micros() == 0 {
        local.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    } else {
        local.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
    };
    if text.ends_with("+00:00") {
        text.truncate(text.len() - "+00:00".len());
        text.push('Z');
    }
    text
}

fn python_date(value: NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

fn user_value(row: &UserLiteRow) -> Json {
    dict(vec![
        ("id", Json::Str(PyStr::new(&row.id.to_string()))),
        ("first_name", Json::Str(PyStr::new(&row.first_name))),
        ("last_name", Json::Str(PyStr::new(&row.last_name))),
        ("avatar", Json::Str(PyStr::new(&row.avatar))),
        ("avatar_url", opt_text(row.avatar_url.as_deref())),
        ("is_bot", Json::Bool(row.is_bot)),
        ("display_name", Json::Str(PyStr::new(&row.display_name))),
    ])
}

fn issue_value(row: &shared::IssueFlatRow) -> Json {
    dict(vec![
        ("id", Json::Str(PyStr::new(&row.id.to_string()))),
        ("name", Json::Str(PyStr::new(&row.name))),
        (
            "description_json",
            json_column_opt(row.description_json.as_ref()),
        ),
        (
            "description_html",
            Json::Str(PyStr::new(&row.description_html)),
        ),
        ("priority", Json::Str(PyStr::new(&row.priority))),
        (
            "start_date",
            row.start_date.map_or(Json::Null, |value| {
                Json::Str(PyStr::new(&python_date(value)))
            }),
        ),
        (
            "target_date",
            row.target_date.map_or(Json::Null, |value| {
                Json::Str(PyStr::new(&python_date(value)))
            }),
        ),
        ("sequence_id", Json::from(i64::from(row.sequence_id))),
        ("sort_order", Json::Float(row.sort_order)),
        ("is_draft", Json::Bool(row.is_draft)),
    ])
}

fn project_value(row: &ProjectLiteRow) -> Json {
    dict(vec![
        ("id", Json::Str(PyStr::new(&row.id.to_string()))),
        ("identifier", Json::Str(PyStr::new(&row.identifier))),
        ("name", Json::Str(PyStr::new(&row.name))),
        ("cover_image", opt_text(row.cover_image.as_deref())),
        ("cover_image_url", opt_text(row.cover_image_url.as_deref())),
        ("logo_props", json_column_opt(row.logo_props.as_ref())),
        ("description", Json::Str(PyStr::new(&row.description))),
    ])
}

fn workspace_value(row: &WorkspaceLiteRow) -> Json {
    dict(vec![
        ("name", Json::Str(PyStr::new(&row.name))),
        ("slug", Json::Str(PyStr::new(&row.slug))),
        ("id", Json::Str(PyStr::new(&row.id.to_string()))),
        ("logo_url", opt_text(row.logo_url.as_deref())),
    ])
}

fn reaction_value(row: &CommentReactionRow, timezone: Tz) -> Json {
    let reaction = &row.reaction;
    dict(vec![
        ("id", Json::Str(PyStr::new(&reaction.id.to_string()))),
        (
            "actor",
            Json::Str(PyStr::new(&reaction.actor_id.to_string())),
        ),
        (
            "comment",
            Json::Str(PyStr::new(&reaction.comment_id.to_string())),
        ),
        ("reaction", Json::Str(PyStr::new(&reaction.reaction))),
        ("display_name", Json::Str(PyStr::new(&row.display_name))),
        (
            "deleted_at",
            reaction.deleted_at.map_or(Json::Null, |value| {
                Json::Str(PyStr::new(&python_datetime(value, timezone)))
            }),
        ),
        (
            "workspace",
            Json::Str(PyStr::new(&reaction.workspace_id.to_string())),
        ),
        (
            "project",
            Json::Str(PyStr::new(&reaction.project_id.to_string())),
        ),
        (
            "created_at",
            Json::Str(PyStr::new(&python_datetime(reaction.created_at, timezone))),
        ),
        (
            "updated_at",
            Json::Str(PyStr::new(&python_datetime(reaction.updated_at, timezone))),
        ),
        ("created_by", opt_uuid(reaction.created_by_id)),
        ("updated_by", opt_uuid(reaction.updated_by_id)),
    ])
}

/// `IssueCommentSerializer(comment).data` as a Python value, in
/// `_readable_fields` order. `is_member` is absent: the write views load the
/// comment without the queryset's annotation.
fn comment_value(row: &CommentRow, reactions: &[CommentReactionRow], timezone: Tz) -> Json {
    let comment = &row.comment;
    let mut entries: Vec<(&str, Json)> = vec![
        ("id", Json::Str(PyStr::new(&comment.id.to_string()))),
        (
            "actor_detail",
            row.actor.as_ref().map_or(Json::Null, user_value),
        ),
        (
            "issue_detail",
            row.issue.as_ref().map_or(Json::Null, issue_value),
        ),
        (
            "project_detail",
            row.project.as_ref().map_or(Json::Null, project_value),
        ),
        (
            "workspace_detail",
            row.workspace.as_ref().map_or(Json::Null, workspace_value),
        ),
        (
            "comment_reactions",
            Json::List(
                reactions
                    .iter()
                    .map(|reaction| reaction_value(reaction, timezone))
                    .collect(),
            ),
        ),
        (
            "created_at",
            Json::Str(PyStr::new(&python_datetime(comment.created_at, timezone))),
        ),
        (
            "updated_at",
            Json::Str(PyStr::new(&python_datetime(comment.updated_at, timezone))),
        ),
        (
            "deleted_at",
            comment.deleted_at.map_or(Json::Null, |value| {
                Json::Str(PyStr::new(&python_datetime(value, timezone)))
            }),
        ),
        (
            "comment_stripped",
            Json::Str(PyStr::new(&comment.comment_stripped)),
        ),
        ("comment_json", json_column(&comment.comment_json)),
        ("comment_html", Json::Str(PyStr::new(&comment.comment_html))),
        (
            "attachments",
            Json::List(
                comment
                    .attachments
                    .iter()
                    .map(|value| Json::Str(PyStr::new(value)))
                    .collect(),
            ),
        ),
        ("access", Json::Str(PyStr::new(&comment.access))),
        (
            "external_source",
            opt_text(comment.external_source.as_deref()),
        ),
        ("external_id", opt_text(comment.external_id.as_deref())),
        (
            "edited_at",
            comment.edited_at.map_or(Json::Null, |value| {
                Json::Str(PyStr::new(&python_datetime(value, timezone)))
            }),
        ),
        ("created_by", opt_uuid(comment.created_by_id)),
        ("updated_by", opt_uuid(comment.updated_by_id)),
        (
            "project",
            Json::Str(PyStr::new(&comment.project_id.to_string())),
        ),
        (
            "workspace",
            Json::Str(PyStr::new(&comment.workspace_id.to_string())),
        ),
        ("description", opt_uuid(comment.description_id)),
        (
            "issue",
            Json::Str(PyStr::new(&comment.issue_id.to_string())),
        ),
        ("actor", opt_uuid(comment.actor_id)),
        ("parent", opt_uuid(comment.parent_id)),
    ];
    entries.shrink_to_fit();
    dict(entries)
}

/// Whether the raw JSON body value differs from a Python string.
fn raw_differs_from(value: &Json, text: &str) -> bool {
    match value {
        Json::Str(value) => value.as_str() != Some(text),
        _ => true,
    }
}
