// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Publishing Celery tasks from a handler (DECISIONS.md D7).
//!
//! `plane-jobs` owns the wire format and the per-call-site argument types
//! (`TASKS.md` records every enqueue site). What a handler still needs is the
//! three values every call site reads off the request — `slug`,
//! `request.user.id` and the entity id — and the publisher itself, which the
//! runtime holds. This module supplies those, so a handler never reaches into
//! `AppState` or invents a `TaskArg` spelling.
//!
//! ## Failure behaviour
//!
//! A view that calls `task.delay(...)` publishes synchronously. Kombu retries
//! three times and then raises `OperationalError`
//! (`celery/app/task.py:897-989`); nothing in Plane's view code catches it, so
//! the family `handle_exception` answers 500
//! (`plane/app/views/base.py:200-239`). [`publish`] does the same. The one
//! exception is `APITokenLogMiddleware`, which wraps its own `delay` in
//! `try/except` and is implemented in the pipeline, not here.

use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, RecentVisitedTask};
use plane_jobs::celery::{CeleryPublisher, Message};
use plane_jobs::ownership::owner;
use plane_jobs::queue::Job;
use plane_jobs::route::{Delivery, dispatch};
use uuid::Uuid;

/// The process's Celery publisher.
///
/// # Errors
///
/// A process without one (a test build) fails the request: Django always has a
/// publisher, so silently dropping the task would be a behaviour change.
pub fn publisher<'a>(ctx: &ViewCtx<'a>) -> Result<&'a CeleryPublisher, ViewError> {
    ctx.celery().ok_or_else(|| {
        ViewError::Unknown(
            "the process has no Celery publisher; Django's .delay() would fail the request"
                .to_string(),
        )
    })
}

/// The Phase 4 routing API: publish to Celery or enqueue to `plane_jobs`
/// according to `plane_jobs::ownership`.
///
/// The Celery branch is the byte-identical coexistence path; it never touches
/// Postgres. The Rust branch inserts the supplied [`Job`] using the request's
/// checked-out connection, or a write-pool connection when the handler did not
/// need one.
///
/// # Errors
///
/// [`ViewError::Unknown`] for a broker failure (Django's 500), a missing
/// publisher, or a Rust-owned task without a database.
pub async fn dispatch_task(
    ctx: &ViewCtx<'_>,
    task: &str,
    message: &Message,
    job: Job,
) -> Result<Option<Uuid>, ViewError> {
    // A Celery-owned task must not touch Postgres, so only check out a pool
    // connection when the ownership table says this task is Rust-owned.
    let delivery = if owner(task).is_rust() {
        match ctx.db {
            Some(client) => {
                dispatch::<tokio_postgres::Client>(
                    task,
                    &job,
                    message,
                    ctx.celery(),
                    Some(&***client),
                )
                .await
            }
            None => {
                let client = ctx.pools.write_pool().get().await.map_err(|error| {
                    ViewError::Unknown(format!("checking out a job connection: {error}"))
                })?;
                dispatch::<tokio_postgres::Client>(
                    task,
                    &job,
                    message,
                    ctx.celery(),
                    Some(&**client),
                )
                .await
            }
        }
    } else {
        dispatch::<tokio_postgres::Client>(task, &job, message, ctx.celery(), None).await
    };
    match delivery.map_err(|error| ViewError::Unknown(error.to_string()))? {
        Delivery::Celery(id) | Delivery::Queued(Some(id)) => Ok(Some(id)),
        Delivery::Queued(None) => Ok(None),
    }
}

/// `request.user.id` as the `uuid.UUID` object the majority of call sites pass
/// (`user_id=request.user.id`).
///
/// A handler mirroring a call site that passes `str(request.user.id)` wraps the
/// value itself: `TaskArg::str(user_id.to_string())`.
///
/// # Errors
///
/// [`ViewError::Key`] for an anonymous caller. Every `recent_visited_task` site
/// is behind `IsAuthenticated`, so this cannot happen after the policy ran.
pub fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user.id().ok_or(ViewError::Key)
}

/// `slug = kwargs["slug"]`, the first argument at every `recent_visited_task`
/// site.
fn slug(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    ctx.kwarg("slug").map(str::to_string).ok_or(ViewError::Key)
}

/// `recent_visited_task.delay(slug=…, project_id=…, entity_name=…,
/// entity_identifier=…, user_id=…)` — the five `ProjectKeyed` sites:
///
/// * `plane/app/views/issue/base.py:165,304` (`entity_name="project"`,
///   `project_id` is the URL kwarg);
/// * `plane/app/views/project/base.py:247`;
/// * `plane/app/views/view/base.py:140` (`project_id=None` for a
///   workspace-level view) and `:376`.
///
/// `entity_identifier` is a `TaskArg` because the sites disagree: the URL kwarg
/// string at these five, `str(issue.id)` at `issue/base.py:1359`.
///
/// # Errors
///
/// A missing `slug` ([`ViewError::Key`]) or a broker failure, as in
/// [`publish`].
pub async fn recent_visited_project(
    ctx: &ViewCtx<'_>,
    entity_name: &str,
    entity_identifier: TaskArg,
    project_id: Option<TaskArg>,
    user_id: TaskArg,
) -> Result<Option<Uuid>, ViewError> {
    let slug = slug(ctx)?;
    let call = RecentVisitedTask::ProjectKeyed {
        slug,
        project_id,
        entity_name: entity_name.to_string(),
        entity_identifier,
        user_id,
    };
    let message = tasks::recent_visited_task(call);
    // The queue payload is derived from the Celery message, so switching the
    // task to Rust is transport-only (CUTOVER.md §2 "Payload equivalence").
    let job = Job::from_message(&message);
    dispatch_task(ctx, message.task(), &message, job).await
}

/// `recent_visited_task.delay(slug=…, entity_name=…, entity_identifier=…,
/// user_id=…, project_id=…)` — the five `EntityKeyed` sites:
///
/// * `plane/app/views/cycle/base.py:469`;
/// * `plane/app/views/issue/base.py:617,1359`;
/// * `plane/app/views/module/base.py:642`;
/// * `plane/app/views/page/base.py:232`.
///
/// Only the keyword order differs from [`recent_visited_project`]; Celery
/// serialises `kwargs` in that order, so a byte comparison of the message (and
/// the worker's `kwargsrepr`) sees it.
///
/// # Errors
///
/// See [`recent_visited_project`].
pub async fn recent_visited_entity(
    ctx: &ViewCtx<'_>,
    entity_name: &str,
    entity_identifier: TaskArg,
    project_id: Option<TaskArg>,
    user_id: TaskArg,
) -> Result<Option<Uuid>, ViewError> {
    let slug = slug(ctx)?;
    let call = RecentVisitedTask::EntityKeyed {
        slug,
        entity_name: entity_name.to_string(),
        entity_identifier,
        user_id,
        project_id,
    };
    let message = tasks::recent_visited_task(call);
    let job = Job::from_message(&message);
    dispatch_task(ctx, message.task(), &message, job).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_jobs::celery::args::argsrepr;

    /// The two helpers must produce the two keyword orders `TASKS.md` records,
    /// not a third one.
    #[test]
    fn the_two_recent_visited_orders_are_the_python_ones() {
        let uuid = Uuid::nil();
        let project = tasks::recent_visited_task(RecentVisitedTask::ProjectKeyed {
            slug: "acme".to_string(),
            project_id: Some(TaskArg::str("p1")),
            entity_name: "project".to_string(),
            entity_identifier: TaskArg::str("p1"),
            user_id: TaskArg::uuid(uuid),
        });
        assert_eq!(
            project.kwargs().iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec![
                "slug",
                "project_id",
                "entity_name",
                "entity_identifier",
                "user_id"
            ]
        );
        let entity = tasks::recent_visited_task(RecentVisitedTask::EntityKeyed {
            slug: "acme".to_string(),
            entity_name: "issue".to_string(),
            entity_identifier: TaskArg::str("i1"),
            user_id: TaskArg::uuid(uuid),
            project_id: Some(TaskArg::str("p1")),
        });
        assert_eq!(
            entity.kwargs().iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec![
                "slug",
                "entity_name",
                "entity_identifier",
                "user_id",
                "project_id"
            ]
        );
        assert_eq!(argsrepr(project.args()), "()");
        assert_eq!(
            project.task(),
            "plane.bgtasks.recent_visited_task.recent_visited_task"
        );
    }
}
