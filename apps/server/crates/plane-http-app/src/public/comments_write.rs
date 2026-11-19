// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The public comment writes: routes 239 (`POST …/comments/`) and 240
//! (`PATCH`/`DELETE …/comments/{pk}/`), `plane/space/views/issue.py:291-395`.
//!
//! ## The public body is not the app body
//!
//! `_public_comment_content` (`:100-103`) reduces `request.data` to
//! `comment_html`/`comment_json` — `access`, `actor`, `parent` and every system
//! field stay server-controlled, and `access="EXTERNAL"` plus `actor` are forced
//! on `save`. A body that is not a mapping is a `TypeError` from `data[key]`
//! (`:103`) → the family 500, before the serializer runs.
//!
//! ## create (`:291-332`)
//!
//! board (`.get(anchor, entity_name="project")`) → comments enabled? (400
//! `Comments are not enabled for this project`) → `_issue_in_board_scope` (404
//! `Issue not found`) → the filtered body → `IssueCommentSerializer(data=…)` →
//! `save(project_id, issue_id, actor, access="EXTERNAL")` (H14 + the
//! `Description` row, one transaction) → `comment.activity.created` with
//! `requested_data=json.dumps(serializer.data)` → a `ProjectPublicMember` row
//! when the caller is not an active member → 201 `serializer.data`.
//!
//! ## partial_update (`:334-366`)
//!
//! Same guards, then `IssueComment.objects.get(pk, issue_id, project_id,
//! workspace_id, access="EXTERNAL", actor=request.user)` (the family 404 on a
//! miss — a comment by anyone else is indistinguishable from a missing one).
//! `current_instance` is rendered **before** the changes, then the serializer
//! saves and the activity carries `json.dumps(data)` (the filtered body, not
//! the serializer's validated data). `edited_at` is **not** written: only the
//! app path does that (`issues/write_comment.rs`), so a public edit keeps NULL.
//!
//! ## destroy (`:368-395`)
//!
//! The same lookup, then `comment.activity.deleted` with
//! `requested_data=json.dumps({"comment_id": str(pk)})` and
//! `current_instance=json.dumps(serializer.data)` — published **before**
//! `comment.delete()` (H03 + the `soft_delete_related_objects` task). 204.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::issue_comment as comment_hook;
use plane_db::models::issue::IssueComment;
use plane_db::{JsonText, hooks::audit::Audit};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::RenderCtx;
use uuid::Uuid;

use crate::common::client;
use crate::issues::sub_resources::sub_common::{
    CommentEntry, CommentReactionRow, CommentRow, IssueFlatRow, ProjectLiteRow, UserLiteRow,
    WorkspaceLiteRow,
};
use crate::issues::sub_resources::sub_queries;
use crate::issues::write_comment_queries as comment_queries;
use crate::public::board;
use crate::public::queries;
use crate::write::description::{self, HtmlValidation};
use crate::write::{self, CharOptions, Serializer};

/// `_public_comment_content`'s key order (`:103`).
const PUBLIC_FIELDS: [&str; 2] = ["comment_html", "comment_json"];

/// Register the three write verbs on the view that already serves
/// `list`/`retrieve` (routes 239/240).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.space.views.issue.IssueCommentPublicViewSet")?
        .action("create", create)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    ctx.kwarg(name)
        .ok_or(ViewError::Key)?
        .parse()
        .map_err(|_| ViewError::Validation)
}

fn epoch() -> i64 {
    Utc::now().timestamp()
}

/// `{"error": …}`.
fn error_body(status: u16, message: &str) -> ViewResponse {
    write::error_body(status, message)
}

/// Django's `IntegrityError` (SQLSTATE class 23) → the family 400.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `IssueCommentPublicViewSet.create` (`plane/space/views/issue.py:291-331`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_comments_enabled {
            return Ok(error_body(400, "Comments are not enabled for this project"));
        }
        if !board::issue_in_board_scope(client, issue_id, &board).await? {
            return Ok(error_body(404, "Issue not found"));
        }
        let Some(project_id) = board.project_id else {
            return Ok(error_body(404, "Issue not found"));
        };

        // `_public_comment_content(request.data)` (`:304`): evaluated for its
        // TypeError on a non-mapping body, before the serializer.
        let _ = public_body(ctx)?;
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
            workspace_id: board.workspace_id,
            comment_stripped: String::new(),
            comment_json: input.json.unwrap_or_else(|| JsonText::new("{}")),
            comment_html: input.html.unwrap_or_else(|| "<p></p>".to_owned()),
            description_id: None,
            attachments: Vec::new(),
            issue_id,
            actor_id: Some(user),
            access: "EXTERNAL".to_owned(),
            external_source: None,
            external_id: None,
            edited_at: None,
            parent_id: None,
        };
        comment_hook::before_save(&mut comment);

        // `IssueComment.save()`'s `transaction.atomic()` block.
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
        // The column-only second save is an update for `BaseModel.save`, so
        // `updated_by` moves in memory but not in the row.
        comment.updated_by_id = Some(user);

        let (mut row, reactions) =
            load_comment(ctx, client, anchor, issue_id, comment.id, user).await?;
        // `serializer.data` renders the **in-memory** instance: `updated_by` is
        // the caller (the second, column-only save did not write it) and
        // `comment_json` keeps the request's key order rather than jsonb's.
        row.comment.updated_by_id = comment.updated_by_id;
        row.comment.comment_json = comment.comment_json.clone();
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let requested = comment_json_string(&row, &reactions, timezone)?;
        publish_activity(
            ctx,
            "comment.activity.created",
            TaskArg::json_text(requested),
            user,
            issue_id,
            project_id,
            TaskArg::Null,
        )
        .await?;

        // `if not ProjectMember.objects.filter(project_id=…, member=…,
        // is_active=True).exists(): ProjectPublicMember.objects.get_or_create`
        // (`:321-329`).
        track_public_member(client, project_id, board.workspace_id, user).await?;

        // `Response(serializer.data, 201)`: the freshly inserted row, so the
        // rendered comment is the in-memory one (the `updated_by` above).
        comment_response(ctx, row, &reactions, 201)
    })
}

/// `IssueCommentPublicViewSet.partial_update` (`plane/space/views/issue.py:334-366`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_comments_enabled {
            return Ok(error_body(400, "Comments are not enabled for this project"));
        }
        let Some(project_id) = board.project_id else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let (mut row, reactions) = load_comment(ctx, client, anchor, issue_id, pk, user).await?;

        // `data = _public_comment_content(request.data)` (`:351`) and
        // `current_instance = json.dumps(IssueCommentSerializer(comment).data)`
        // (`:352`) — both before the serializer.
        let data = public_body(ctx)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let current_instance = comment_json_string(&row, &reactions, timezone)?;
        let requested_data = django_json(&Json::Dict(data.clone()))?;

        let input = match validate_comment(ctx)? {
            Ok(input) => input,
            Err(response) => return Ok(response),
        };
        let old = row.comment.clone();
        if let Some(html) = input.html {
            row.comment.comment_html = html;
        }
        if let Some(json) = input.json {
            row.comment.comment_json = json;
        }
        // The public path never writes `edited_at` (`:353-355`).
        comment_hook::before_save(&mut row.comment);
        row.comment.updated_at = write::now();
        row.comment.updated_by_id = Audit::on_update(Some(user));
        let changes = comment_hook::description_changes(&old, &row.comment);
        save_comment(client, &row.comment, &changes).await?;

        publish_activity(
            ctx,
            "comment.activity.updated",
            TaskArg::json_text(requested_data),
            user,
            issue_id,
            project_id,
            TaskArg::json_text(current_instance),
        )
        .await?;

        // `serializer.data` after `save()`: the instance itself, so the
        // reactions are re-read for the row.
        let reactions = comment_reactions(client, &[pk]).await?;
        comment_response(ctx, row, &reactions, 200)
    })
}

/// `IssueCommentPublicViewSet.destroy` (`plane/space/views/issue.py:368-395`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_comments_enabled {
            return Ok(error_body(400, "Comments are not enabled for this project"));
        }
        let Some(project_id) = board.project_id else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let (row, reactions) = load_comment(ctx, client, anchor, issue_id, pk, user).await?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let current_instance = comment_json_string(&row, &reactions, timezone)?;
        let mut requested = PyDict::new();
        requested.insert(
            PyStr::new("comment_id"),
            Json::Str(PyStr::new(&pk.to_string())),
        );
        publish_activity(
            ctx,
            "comment.activity.deleted",
            TaskArg::json_text(django_json(&Json::Dict(requested))?),
            user,
            issue_id,
            project_id,
            TaskArg::json_text(current_instance),
        )
        .await?;

        // `comment.delete()`: H03 then the cascade task (`:394`).
        write::soft_delete(
            client,
            "issue_comments",
            "id",
            pk,
            Audit::on_update(Some(user)),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuecomment", pk).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Request body and validation
// ---------------------------------------------------------------------------

/// `{key: data[key] for key in ("comment_html", "comment_json")
/// if key in data}` (`plane/space/views/issue.py:100-103`).
///
/// A non-mapping body raises `TypeError: '<type>' object is not subscriptable`
/// before the serializer — the family 500. A form body is a `QueryDict`, whose
/// `in`/`[]` work.
fn public_body(ctx: &ViewCtx<'_>) -> Result<PyDict, ViewError> {
    match ctx.data()? {
        Data::Json(Json::Dict(map)) => {
            let mut out = PyDict::new();
            for key in PUBLIC_FIELDS {
                if let Some(value) = map.get(key) {
                    out.insert(PyStr::new(key), value.clone());
                }
            }
            Ok(out)
        }
        Data::Form(query) => {
            let mut out = PyDict::new();
            for key in PUBLIC_FIELDS {
                if let Some(value) = query.get(key) {
                    out.insert(PyStr::new(key), Json::Str(PyStr::new(value)));
                }
            }
            Ok(out)
        }
        Data::Json(other) => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not subscriptable",
            python_type_name(other)
        ))),
    }
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

#[derive(Default, Debug)]
struct CommentInput {
    html: Option<String>,
    json: Option<JsonText>,
}

/// `IssueCommentSerializer(data=…)` + `validate()` for the two public fields.
///
/// `IssueCommentSerializer.FIELDS` visits `comment_json` before
/// `comment_html`, so the error keys come out in that order.
fn validate_comment(ctx: &ViewCtx<'_>) -> Result<Result<CommentInput, ViewResponse>, ViewError> {
    let mut serializer = Serializer::new(ctx, false)?;
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
    if let Err(errors) = serializer.finish() {
        return Ok(Err(errors.into_response()));
    }
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
    Ok(Ok(CommentInput { html, json }))
}

/// A keyed error body whose value is a bare string (`validate()`'s
/// `ValidationError({"comment_html": "…"})`).
fn single_string_error(field: &str, message: &str) -> ViewResponse {
    let body = plane_query::json::Json::Object(vec![(
        field.to_owned(),
        plane_query::json::Json::string(message),
    )])
    .to_drf();
    ViewResponse::raw(400, APPLICATION_JSON, body)
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// `IssueComment.objects.get(pk, issue_id, project_id, workspace_id,
/// access="EXTERNAL", actor=request.user)` (`:343-350`, `:377-384`) plus the
/// comment's reactions.
async fn load_comment(
    ctx: &ViewCtx<'_>,
    client: &Client,
    anchor: &str,
    issue_id: Uuid,
    pk: Uuid,
    user: Uuid,
) -> Result<(CommentRow, Vec<CommentReactionRow>), ViewError> {
    let board = board::require_published_board(client, anchor).await?;
    let Some(project_id) = board.project_id else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let statement = queries::PUBLIC_COMMENT_ROW_BY_ID.prepare(client).await?;
    let Some(row) = client
        .query_opt(
            &statement,
            &[&pk, &board.workspace_id, &project_id, &issue_id, &user],
        )
        .await?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let row = CommentRow::from_row(&row, 0, false)?;
    let reactions = comment_reactions(client, &[pk]).await?;
    let _ = ctx;
    Ok((row, reactions))
}

/// `comment.comment_reactions.all()`
/// (`CommentReaction.Meta.ordering = ("-created_at",)`).
async fn comment_reactions(
    client: &Client,
    comments: &[Uuid],
) -> Result<Vec<CommentReactionRow>, ViewError> {
    if comments.is_empty() {
        return Ok(Vec::new());
    }
    let statement = sub_queries::COMMENT_REACTIONS.prepare(client).await?;
    let rows = client.query(&statement, &[&comments]).await?;
    rows.iter()
        .map(|row| CommentReactionRow::from_row(row, 0).map_err(ViewError::from))
        .collect()
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// `IssueComment.save()`'s atomic block: INSERT, `Description.objects.create`,
/// then the column-only `description_id` save.
async fn insert_comment(
    client: &Client,
    comment: &mut IssueComment,
    user: Uuid,
) -> Result<(), ViewError> {
    let statement = comment_queries::COMMENT_INSERT.prepare(client).await?;
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
    let statement = comment_queries::DESCRIPTION_INSERT.prepare(client).await?;
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
    let statement = comment_queries::COMMENT_SET_DESCRIPTION
        .prepare(client)
        .await?;
    client
        .execute(&statement, &[&description_id, &comment.id])
        .await
        .map_err(integrity)?;
    Ok(())
}

/// `IssueComment.save()`'s UPDATE plus the `Description` sync, one transaction.
async fn save_comment(
    client: &Client,
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
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

async fn save_comment_rows(
    client: &Client,
    comment: &IssueComment,
    changes: &comment_hook::DescriptionChanges,
) -> Result<(), ViewError> {
    let statement = comment_queries::COMMENT_UPDATE.prepare(client).await?;
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

/// `ProjectPublicMember.objects.get_or_create(...)` when the caller is not an
/// active project member (`:321-329`).
async fn track_public_member(
    client: &Client,
    project_id: Uuid,
    workspace_id: Uuid,
    user: Uuid,
) -> Result<(), ViewError> {
    let statement = queries::PUBLIC_ACTIVE_PROJECT_MEMBER
        .prepare(client)
        .await?;
    if client
        .query_opt(&statement, &[&project_id, &user])
        .await?
        .is_some()
    {
        return Ok(());
    }
    let statement = queries::PUBLIC_PUBLIC_MEMBER_LOOKUP.prepare(client).await?;
    if client
        .query_opt(&statement, &[&project_id, &user])
        .await?
        .is_some()
    {
        return Ok(());
    }
    let now = write::now();
    let statement = queries::PUBLIC_PUBLIC_MEMBER_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Some(user),
                &Uuid::new_v4(),
                &project_id,
                &workspace_id,
                &user,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering and tasks
// ---------------------------------------------------------------------------

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

/// `json.dumps(serializer.data, cls=DjangoJSONEncoder)` — `DjangoJSONEncoder`
/// renders a datetime with milliseconds and `Z`, unlike DRF's renderer.
fn comment_json_string(
    row: &CommentRow,
    reactions: &[CommentReactionRow],
    timezone: Tz,
) -> Result<String, ViewError> {
    django_json(&comment_value(row, reactions, timezone))
}

/// `json.dumps(value, cls=DjangoJSONEncoder)`.
fn django_json(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `issue_activity.delay(...)` with the seven base kwargs.
async fn publish_activity(
    ctx: &ViewCtx<'_>,
    activity_type: &str,
    requested_data: TaskArg,
    user: Uuid,
    issue_id: Uuid,
    project_id: Uuid,
    current_instance: TaskArg,
) -> Result<(), ViewError> {
    let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
        activity_type,
        requested_data,
        user.to_string(),
        TaskArg::str(issue_id.to_string()),
        TaskArg::str(project_id.to_string()),
        current_instance,
        epoch(),
    )));
    crate::common::jobs::dispatch_task(
        ctx,
        activity.task(),
        &activity,
        plane_jobs::queue::Job::from_message(&activity),
    )
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Serializer data as Python JSON (`json.dumps(serializer.data, …)`)
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

fn issue_value(row: &IssueFlatRow) -> Json {
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
                let text = python_date(value);
                Json::Str(PyStr::new(&text))
            }),
        ),
        (
            "target_date",
            row.target_date.map_or(Json::Null, |value| {
                let text = python_date(value);
                Json::Str(PyStr::new(&text))
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
                Json::Str(PyStr::from_string(python_datetime(value, timezone)))
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
            Json::Str(PyStr::from_string(python_datetime(
                reaction.created_at,
                timezone,
            ))),
        ),
        (
            "updated_at",
            Json::Str(PyStr::from_string(python_datetime(
                reaction.updated_at,
                timezone,
            ))),
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
    dict(vec![
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
            Json::Str(PyStr::from_string(python_datetime(
                comment.created_at,
                timezone,
            ))),
        ),
        (
            "updated_at",
            Json::Str(PyStr::from_string(python_datetime(
                comment.updated_at,
                timezone,
            ))),
        ),
        (
            "deleted_at",
            comment.deleted_at.map_or(Json::Null, |value| {
                Json::Str(PyStr::from_string(python_datetime(value, timezone)))
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
                Json::Str(PyStr::from_string(python_datetime(value, timezone)))
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
    ])
}
