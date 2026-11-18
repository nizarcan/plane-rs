// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueVotePublicViewSet.create` / `.destroy` (`plane/space/views/issue.py`:
//! `:624-663` and `:665-683`, route 245's write halves).
//!
//! ## create (`:624-663`)
//!
//! 1. `DeployBoard.objects.get(anchor, entity_name="project")` — the family 404
//!    on a miss ([`super::board::require_published_board`]).
//! 2. `is_votes_enabled` off → 400 `{"error": "Votes are not enabled for this
//!    project board"}`.
//! 3. `_issue_in_board_scope(issue_id, board)` → 404 `{"error": "Issue not
//!    found"}`.
//! 4. `IssueVote.objects.get_or_create(actor_id, project_id, issue_id)` — no
//!    `workspace` in the lookup; `ProjectBaseModel.save` derives it from the
//!    project on the create branch.
//! 5. `ProjectPublicMember.objects.get_or_create(project_id, member)` when the
//!    caller is not an active member of the project.
//! 6. `issue_vote.vote = request.data.get("vote", 1)` — a **raw** assignment
//!    (there is no serializer), then a full `save()`: `updated_at` moves,
//!    `updated_by` becomes the caller (H01) and `_sync_completed_at` does not
//!    run (not an issue). `"vote"` is written as-is, so `2` is accepted past
//!    the model's `choices`, a non-numeric string is a `ValueError` 500
//!    (`IntegerField.get_prep_value`) and `null` violates NOT NULL
//!    (`IntegrityError` → the family 400 `{"error": "The payload is not
//!    valid"}`).
//! 7. `issue_activity.delay(type="issue_vote.activity.created",
//!    requested_data=json.dumps(request.data), …, epoch=…)` — the seven base
//!    kwargs only (`IssueActivityShape::Bare`).
//! 8. `IssueVoteSerializer(issue_vote)` → 201.
//!
//! ## destroy (`:665-683`)
//!
//! The board is resolved again but `is_votes_enabled` is **not** read
//! (`PUBLIC_INSTANCES.md` §5), then
//! `IssueVote.objects.get(issue_id, actor_id, project_id, workspace_id)` — a
//! miss is the family 404. The activity is published **before** the soft
//! delete, with `current_instance=json.dumps({"vote": str(vote), "identifier":
//! str(id)})`, and the delete is H03 plus the `soft_delete_related_objects`
//! task (`db`/`issuevote`). 204.

use chrono::Utc;
use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::issue::IssueVote;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::RenderCtx;
use plane_serializers::generated::app_issue::IssueVoteSerializer;
use uuid::Uuid;

use crate::common::client;
use crate::issues::sub_resources::sub_common::UserLiteRow;
use crate::public::board;
use crate::public::queries;
use crate::write::{self, Audit};

/// Register the two write verbs on the view that already serves `list`
/// (route 245).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.space.views.issue.IssueVotePublicViewSet")?
        .action("create", create)?
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

/// `int(timezone.now().timestamp())`.
fn epoch() -> i64 {
    Utc::now().timestamp()
}

/// `IssueVote.objects.get_or_create(...)`, then `issue_vote.vote = …;
/// issue_vote.save()`.
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if !board.is_votes_enabled {
            return Ok(write::error_body(
                400,
                "Votes are not enabled for this project board",
            ));
        }
        if !board::issue_in_board_scope(client, issue_id, &board).await? {
            return Ok(write::error_body(404, "Issue not found"));
        }

        // `IssueVote.objects.get_or_create(actor_id=…, project_id=…,
        // issue_id=…)` (`:637-641`).
        let statement = queries::PUBLIC_VOTE_FOR_ACTOR.prepare(client).await?;
        let existing = client
            .query_opt(&statement, &[&user, &board.project_id, &issue_id])
            .await?;
        let vote_id = match existing {
            Some(row) => row.get::<_, Uuid>(0),
            None => insert_vote(client, &board, issue_id, user).await?,
        };

        // `ProjectPublicMember.objects.get_or_create(project_id=…,
        // member=request.user)` when the caller is not an active member
        // (`:643-650`).
        track_public_member(client, &board, user).await?;

        // `issue_vote.vote = request.data.get("vote", 1); issue_vote.save()`
        // (`:651-652`): a full save, so H01 and `auto_now` run. The body value
        // is read **after** the get_or_create, so a bad value still leaves the
        // row the lookup inserted behind.
        let vote = vote_value(ctx)?;
        let statement = queries::PUBLIC_VOTE_BY_ID.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&vote_id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut issue_vote = IssueVote::from_row(&row, 0)?;
        let now = write::now();
        if let Some(vote) = vote {
            issue_vote.vote = vote;
        }
        issue_vote.updated_at = now;
        issue_vote.updated_by_id = Some(user);
        // `ProjectBaseModel.save` re-derives the workspace from the project.
        let statement = queries::PUBLIC_VOTE_UPDATE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &issue_vote.created_at,
                    &issue_vote.updated_at,
                    &issue_vote.created_by_id,
                    &issue_vote.updated_by_id,
                    &issue_vote.deleted_at,
                    &issue_vote.id,
                    &issue_vote.project_id,
                    &issue_vote.workspace_id,
                    &issue_vote.actor_id,
                    &issue_vote.issue_id,
                    &vote,
                    &issue_vote.id,
                ],
            )
            .await
            .map_err(integrity)?;

        // `issue_activity.delay(...)` (`:653-661`).
        publish_activity(
            ctx,
            "issue_vote.activity.created",
            write::request_data_json_text(ctx)?,
            user,
            &issue_id,
            board.project_id,
            TaskArg::Null,
        )
        .await?;

        // `IssueVoteSerializer(issue_vote)` → 201 (`:662-663`).
        let statement = queries::PUBLIC_VOTE_BY_ID.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&issue_vote.id]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let vote_row = IssueVote::from_row(&row, 0)?;
        let actor = UserLiteRow::from_row(&row, IssueVote::COLUMN_COUNT)?;
        let actor_serializer = actor.as_ref().map(UserLiteRow::serializer);
        let serializer = IssueVoteSerializer {
            issue: vote_row.issue_id,
            vote: i64::from(vote_row.vote),
            workspace: vote_row.workspace_id,
            project: vote_row.project_id,
            actor: vote_row.actor_id,
            actor_detail: actor_serializer.as_ref(),
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `IssueVotePublicViewSet.destroy` (`plane/space/views/issue.py:665-683`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let issue_id = uuid_kwarg(ctx, "issue_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        // No `is_votes_enabled` check on this path (`:665-672`).
        let statement = queries::PUBLIC_VOTE_TARGET.prepare(client).await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&issue_id, &user, &board.project_id, &board.workspace_id],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let vote_id: Uuid = row.get(0);
        let vote: i32 = row.get(1);

        // `json.dumps({"vote": str(issue_vote.vote), "identifier":
        // str(issue_vote.id)})` (`:679`).
        let mut current = PyDict::new();
        current.insert(PyStr::new("vote"), Json::Str(PyStr::new(&vote.to_string())));
        current.insert(
            PyStr::new("identifier"),
            Json::Str(PyStr::new(&vote_id.to_string())),
        );
        let current_instance = django_json(&Json::Dict(current))?;
        publish_activity(
            ctx,
            "issue_vote.activity.deleted",
            TaskArg::Null,
            user,
            &issue_id,
            board.project_id,
            TaskArg::json_text(current_instance),
        )
        .await?;

        // `issue_vote.delete()`: H03 then the cascade task.
        write::soft_delete(
            client,
            "issue_votes",
            "id",
            vote_id,
            Audit::on_update(Some(user)),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuevote", vote_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

async fn insert_vote(
    client: &Client,
    board: &plane_db::models::deploy_board::DeployBoard,
    issue_id: Uuid,
    user: Uuid,
) -> Result<Uuid, ViewError> {
    let vote_id = Uuid::new_v4();
    let now = write::now();
    let statement = queries::PUBLIC_VOTE_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Some(user),
                &vote_id,
                &board.project_id,
                &board.workspace_id,
                &user,
                &issue_id,
                &1_i32,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(vote_id)
}

/// `if not ProjectMember.objects.filter(project_id=…, member=request.user,
/// is_active=True).exists(): ProjectPublicMember.objects.get_or_create(…)`
/// (`:643-650`).
async fn track_public_member(
    client: &Client,
    board: &plane_db::models::deploy_board::DeployBoard,
    user: Uuid,
) -> Result<(), ViewError> {
    let statement = queries::PUBLIC_ACTIVE_PROJECT_MEMBER
        .prepare(client)
        .await?;
    let member = client
        .query_opt(&statement, &[&board.project_id, &user])
        .await?;
    if member.is_some() {
        return Ok(());
    }
    let statement = queries::PUBLIC_PUBLIC_MEMBER_LOOKUP.prepare(client).await?;
    if client
        .query_opt(&statement, &[&board.project_id, &user])
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
                &board.project_id,
                &board.workspace_id,
                &user,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(())
}

/// `request.data.get("vote", 1)` as the value `IntegerField.get_prep_value`
/// would hand Postgres: `None` is a JSON `null` and reaches the column as NULL.
fn vote_value(ctx: &ViewCtx<'_>) -> Result<Option<i32>, ViewError> {
    let value = match ctx.data()? {
        plane_drf::parsers::Data::Json(Json::Dict(map)) => map.get("vote").cloned(),
        plane_drf::parsers::Data::Form(query) => {
            query.get("vote").map(|text| Json::Str(PyStr::new(text)))
        }
        plane_drf::parsers::Data::Json(_) => None,
    };
    let Some(value) = value else {
        return Ok(Some(1));
    };
    let out_of_range = || overflow_error("value out of range for type integer");
    match &value {
        Json::Int(number) => i32::try_from(
            number
                .as_i64()
                .ok_or_else(|| overflow_error("int too large to convert to int"))?,
        )
        .map(Some)
        .map_err(|_| out_of_range()),
        Json::Bool(flag) => Ok(Some(i32::from(*flag))),
        Json::Float(number) => i32::try_from(*number as i64)
            .map(Some)
            .map_err(|_| out_of_range()),
        Json::Str(text) => {
            let Some(text) = text.as_str() else {
                return Ok(Some(1));
            };
            match plane_core::py::int(text) {
                Ok(value) => i32::try_from(value).map(Some).map_err(|_| out_of_range()),
                Err(plane_core::py::PyIntError::Invalid) => Err(ViewError::Unknown(format!(
                    "ValueError: invalid literal for int() with base 10: {text:?}"
                ))),
                Err(plane_core::py::PyIntError::Overflow) => Err(out_of_range()),
            }
        }
        // `IntegerField.get_prep_value(None)` is None: the UPDATE binds NULL
        // and NOT NULL raises the family 400.
        Json::Null => Ok(None),
        Json::List(_) | Json::Dict(_) => Err(ViewError::Unknown(
            "TypeError: int() argument must be a string, a bytes-like object or a real number"
                .to_owned(),
        )),
    }
}

fn overflow_error(message: &str) -> ViewError {
    ViewError::Unknown(format!("DataError: {message}"))
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
    issue_id: &Uuid,
    project_id: Option<Uuid>,
    current_instance: TaskArg,
) -> Result<(), ViewError> {
    // `str(project_deploy_board.project_id)`: Python renders a NULL FK as the
    // literal `"None"`.
    let project = project_id.map_or_else(|| "None".to_owned(), |id| id.to_string());
    let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
        activity_type,
        requested_data,
        user.to_string(),
        TaskArg::str(issue_id.to_string()),
        TaskArg::str(project),
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

/// Django's `IntegrityError` (SQLSTATE class 23) → the family 400.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}
