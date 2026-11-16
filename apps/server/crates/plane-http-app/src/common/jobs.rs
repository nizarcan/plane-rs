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

/// `task.delay(...)`: render the message and publish it, returning Celery's
/// task id.
///
/// # Errors
///
/// [`ViewError::Unknown`] when the broker is unreachable after the publish
/// retries — the 500 Django produces.
pub async fn publish(ctx: &ViewCtx<'_>, message: &Message) -> Result<Uuid, ViewError> {
    Ok(publisher(ctx)?.publish(message).await?)
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
) -> Result<Uuid, ViewError> {
    let call = RecentVisitedTask::ProjectKeyed {
        slug: slug(ctx)?,
        project_id,
        entity_name: entity_name.to_string(),
        entity_identifier,
        user_id,
    };
    publish(ctx, &tasks::recent_visited_task(call)).await
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
) -> Result<Uuid, ViewError> {
    let call = RecentVisitedTask::EntityKeyed {
        slug: slug(ctx)?,
        entity_name: entity_name.to_string(),
        entity_identifier,
        user_id,
        project_id,
    };
    publish(ctx, &tasks::recent_visited_task(call)).await
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
