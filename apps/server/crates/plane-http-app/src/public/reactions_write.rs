// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The public reaction writes: routes 241/242 (`IssueReactionPublicViewSet`)
//! and 243/244 (`CommentReactionPublicViewSet`), `plane/space/views/issue.py`.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | 241 | `IssueReactionPublicViewSet.create` | `:422-461` |
//! | 242 | `IssueReactionPublicViewSet.destroy` | `:463-491` |
//! | 243 | `CommentReactionPublicViewSet.create` | `:519-564` |
//! | 244 | `CommentReactionPublicViewSet.destroy` | `:566-600` |
//!
//! Every handler starts with the board (`DeployBoard.objects.get(anchor,
//! entity_name="project")` → the family 404), then `is_reactions_enabled`
//! (400), then the scope predicate — `_issue_in_board_scope`
//! (`:431-433`, 404 `Issue not found`) or `_comment_in_board_scope`
//! (`:528-530`, 404 `Comment not found`). The message differs per collection:
//! `"Reactions are not enabled for this project board"` for issues and
//! `"Reactions are not enabled for this board"` for comments.
//!
//! ## Create
//!
//! `IssueReactionSerializer` accepts `reaction` plus `created_by`/`updated_by`
//! (its `read_only_fields` do not list them); `CommentReactionSerializer` takes
//! `reaction` only. `serializer.save(project_id=…, issue_id/comment_id=…,
//! actor=request.user)` runs `ProjectBaseModel.save`, which derives the
//! workspace from the project. A non-member caller then gets a
//! `ProjectPublicMember` row, and the activity
//! (`issue_reaction.activity.created` / `comment_reaction.activity.created`,
//! seven base kwargs) carries `json.dumps(request.data)` — the raw body, so
//! `created_by`.
//!
//! A duplicate reaction is an **uncaught** `IntegrityError`
//! (`space/views/base.py:91-95`) → 400 `{"error": "The payload is not valid"}`,
//! unlike the app route's comment-reaction branch, which catches it.
//!
//! ## Destroy
//!
//! The lookup binds the board's workspace and project, the URL object, the
//! reaction code and the caller (`:474-480`, `:576-583`) — a miss is the
//! family 404. The activity is published before the soft delete;
//! `current_instance` is `{"reaction": code, "identifier": id}` plus
//! `"comment_id"` on the comment route. The delete is H03 plus the cascade task
//! (`db`/`issuereaction`, `db`/`commentreaction`).

use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::issue::IssueReaction;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::RenderCtx;
use plane_serializers::generated::app_issue::IssueReactionSerializer;
use uuid::Uuid;

use crate::common::client;
use crate::issues::sub_resources::sub_common::{CommentReactionRow, UserLiteRow};
use crate::issues::write_comment_queries as comment_queries;
use crate::public::board;
use crate::public::queries;
use crate::write::{self, Audit, CharOptions, Serializer};

/// Register the four write verbs on the two views that already serve `list`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.space.views.issue.IssueReactionPublicViewSet")?
        .action("create", issue_create)?
        .action("destroy", issue_destroy)?;
    registry
        .viewset("plane.space.views.issue.CommentReactionPublicViewSet")?
        .action("create", comment_create)?
        .action("destroy", comment_destroy)?;
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
    chrono::Utc::now().timestamp()
}

/// Django's `IntegrityError` (SQLSTATE class 23) → the family 400
/// `{"error": "The payload is not valid"}`.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `IssueReactionPublicViewSet.create` (`plane/space/views/issue.py:422-461`).
pub fn issue_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_reactions_enabled {
            return Ok(write::error_body(
                400,
                "Reactions are not enabled for this project board",
            ));
        }
        if !board::issue_in_board_scope(client, issue_id, &board).await? {
            return Ok(write::error_body(404, "Issue not found"));
        }

        let reaction = match validate_reaction(ctx, client, true).await? {
            Ok(reaction) => reaction,
            Err(response) => return Ok(response),
        };

        let reaction_id = Uuid::new_v4();
        let now = write::now();
        let statement = comment_queries::ISSUE_REACTION_INSERT
            .prepare(client)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &reaction_id,
                    &board.project_id,
                    &board.workspace_id,
                    &user,
                    &issue_id,
                    &reaction,
                ],
            )
            .await
            .map_err(integrity)?;

        track_public_member(client, &board, user).await?;

        let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
            "issue_reaction.activity.created",
            write::request_data_json_text(ctx)?,
            user.to_string(),
            TaskArg::str(issue_id.to_string()),
            TaskArg::str(project_str(board.project_id)),
            TaskArg::Null,
            epoch(),
        )));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        let statement = comment_queries::ISSUE_REACTION_BY_ID
            .prepare(client)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&reaction_id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction_row = IssueReaction::from_row(&row, 0)?;
        let actor = UserLiteRow::from_row(&row, IssueReaction::COLUMN_COUNT)?;
        let actor_serializer = actor.as_ref().map(UserLiteRow::serializer);
        let serializer = IssueReactionSerializer {
            id: reaction_row.id,
            actor_detail: actor_serializer.as_ref(),
            created_at: reaction_row.created_at,
            updated_at: reaction_row.updated_at,
            deleted_at: reaction_row.deleted_at,
            reaction: &reaction_row.reaction,
            created_by: reaction_row.created_by_id,
            updated_by: reaction_row.updated_by_id,
            project: reaction_row.project_id,
            workspace: reaction_row.workspace_id,
            actor: reaction_row.actor_id,
            issue: reaction_row.issue_id,
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `IssueReactionPublicViewSet.destroy` (`plane/space/views/issue.py:463-491`).
pub fn issue_destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let reaction_code = ctx.kwarg("reaction_code").ok_or(ViewError::Key)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_reactions_enabled {
            return Ok(write::error_body(
                400,
                "Reactions are not enabled for this project board",
            ));
        }
        let statement = queries::PUBLIC_ISSUE_REACTION_TARGET
            .prepare(client)
            .await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[
                    &board.workspace_id,
                    &board.project_id,
                    &issue_id,
                    &reaction_code,
                    &user,
                ],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction_id: Uuid = row.get(0);

        let mut current = PyDict::new();
        current.insert(PyStr::new("reaction"), Json::Str(PyStr::new(reaction_code)));
        current.insert(
            PyStr::new("identifier"),
            Json::Str(PyStr::new(&reaction_id.to_string())),
        );
        let current_instance = django_json(&Json::Dict(current))?;
        publish_activity(
            ctx,
            "issue_reaction.activity.deleted",
            TaskArg::Null,
            user,
            TaskArg::str(issue_id.to_string()),
            board.project_id,
            TaskArg::json_text(current_instance),
        )
        .await?;

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

/// `CommentReactionPublicViewSet.create` (`plane/space/views/issue.py:519-564`).
pub fn comment_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let comment_id = uuid_kwarg(ctx, "comment_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_reactions_enabled {
            return Ok(write::error_body(
                400,
                "Reactions are not enabled for this board",
            ));
        }
        if !board::comment_in_board_scope(client, comment_id, &board).await? {
            return Ok(write::error_body(404, "Comment not found"));
        }

        let reaction = match validate_reaction(ctx, client, false).await? {
            Ok(reaction) => reaction,
            Err(response) => return Ok(response),
        };

        let reaction_id = Uuid::new_v4();
        let now = write::now();
        let statement = comment_queries::COMMENT_REACTION_INSERT
            .prepare(client)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &reaction_id,
                    &board.project_id,
                    &board.workspace_id,
                    &user,
                    &comment_id,
                    &reaction,
                ],
            )
            .await
            .map_err(integrity)?;

        track_public_member(client, &board, user).await?;

        let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
            "comment_reaction.activity.created",
            write::request_data_json_text(ctx)?,
            user.to_string(),
            TaskArg::Null,
            TaskArg::str(project_str(board.project_id)),
            TaskArg::Null,
            epoch(),
        )));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        let statement = comment_queries::COMMENT_REACTION_BY_ID
            .prepare(client)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&reaction_id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction_row = CommentReactionRow::from_row(&row, 0)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body =
            plane_serializers::render(&reaction_row.serializer(), &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `CommentReactionPublicViewSet.destroy` (`plane/space/views/issue.py:566-600`).
pub fn comment_destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let comment_id = uuid_kwarg(ctx, "comment_id")?;
        let reaction_code = ctx.kwarg("reaction_code").ok_or(ViewError::Key)?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_reactions_enabled {
            return Ok(write::error_body(
                400,
                "Reactions are not enabled for this board",
            ));
        }
        // `CommentReaction.objects.get(project_id=…, workspace_id=…,
        // comment_id=…, comment__access="EXTERNAL", reaction=…, actor=…)`
        // (`:576-583`).
        let statement = queries::PUBLIC_COMMENT_REACTION_TARGET
            .prepare(client)
            .await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[
                    &board.project_id,
                    &board.workspace_id,
                    &comment_id,
                    &reaction_code,
                    &user,
                ],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let reaction_id: Uuid = row.get(0);

        let mut current = PyDict::new();
        current.insert(PyStr::new("reaction"), Json::Str(PyStr::new(reaction_code)));
        current.insert(
            PyStr::new("identifier"),
            Json::Str(PyStr::new(&reaction_id.to_string())),
        );
        current.insert(
            PyStr::new("comment_id"),
            Json::Str(PyStr::new(&comment_id.to_string())),
        );
        let current_instance = django_json(&Json::Dict(current))?;
        publish_activity(
            ctx,
            "comment_reaction.activity.deleted",
            TaskArg::Null,
            user,
            TaskArg::Null,
            board.project_id,
            TaskArg::json_text(current_instance),
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

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// `str(board.project_id)` — Python renders a NULL FK as the literal `"None"`.
fn project_str(project_id: Option<Uuid>) -> String {
    project_id.map_or_else(|| "None".to_owned(), |id| id.to_string())
}

/// `IssueReactionSerializer(data=…)` / `CommentReactionSerializer(data=…)`
/// + `is_valid()`.
///
/// `IssueReactionSerializer.FIELDS` visits `reaction`, then `created_by`, then
/// `updated_by` (the model's `__all__` order, `SERIALIZERS.md`); the comment
/// serializer has only `reaction`.
async fn validate_reaction(
    ctx: &ViewCtx<'_>,
    client: &Client,
    with_audit_fields: bool,
) -> Result<Result<String, ViewResponse>, ViewError> {
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
        return Err(ViewError::Unknown("reaction is required".to_string()));
    };
    Ok(Ok(reaction))
}

/// `ProjectPublicMember.objects.get_or_create(...)` when the caller is not an
/// active project member (`:442-450`, `:539-547`).
async fn track_public_member(
    client: &Client,
    board: &plane_db::models::deploy_board::DeployBoard,
    user: Uuid,
) -> Result<(), ViewError> {
    let Some(project_id) = board.project_id else {
        // `ProjectMember.objects.filter(project_id=None, …)` is never true,
        // so the create runs with a NULL project and NOT NULL rejects it.
        return Err(ViewError::Unknown(
            "IntegrityError: null value in column \"project_id\" violates not-null constraint"
                .to_owned(),
        ));
    };
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
                &board.workspace_id,
                &user,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(())
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
    issue_id: TaskArg,
    project_id: Option<Uuid>,
    current_instance: TaskArg,
) -> Result<(), ViewError> {
    let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
        activity_type,
        requested_data,
        user.to_string(),
        issue_id,
        TaskArg::str(project_str(project_id)),
        current_instance,
        epoch(),
    )));
    crate::common::jobs::dispatch_task(
        ctx,
        activity.task(),
        &activity,
        plane_jobs::queue::Job::new(activity.task()),
    )
    .await
    .map(|_| ())
}
