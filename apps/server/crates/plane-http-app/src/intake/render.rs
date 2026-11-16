// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Rendering and the handful of shapes the `intake` handlers share.
//!
//! Every serializer this group renders is a plain `ModelSerializer` — none of
//! them accepts `fields=`/`expand=` (DRF raises `TypeError` for both on a
//! `BaseSerializer`, see `rest_framework/serializers.py:465-485`). The view
//! therefore constructs them with no mask and no expansions, and
//! [`plain_context`] is the only context they are rendered with.
//!
//! This matters for the nested serializers: `IntakeIssueDetailSerializer.issue`
//! is a `IssueDetailSerializer(read_only=True)` — a `DynamicBaseSerializer`
//! built without `fields=`/`expand=` — so the request's parameters must not be
//! forwarded into it, or a `?fields=id` would strip the nested issue's keys
//! even though DRF never passes the mask down (`SERIALIZERS.md`, "manual").

use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;

/// The context a plain serializer renders under: the request timezone and no
/// field mask, which is what `IntakeSerializer(instance)` and friends get.
///
pub fn plain_context<'a>(ctx: &'a ViewCtx<'a>) -> RenderCtx<'a> {
    let timezone: &'a Tz = match ctx.timezone {
        Some(timezone) => timezone,
        None => &chrono_tz::UTC,
    };
    RenderCtx::new(timezone, None, &[])
}

/// A handler for an action the runtime's `BaseViewSet` guard always refuses
/// (`plane/app/views/base.py:103-198`): DRF's generic mixin serves the action
/// and no permission class authorizes it, so the policy answers
/// `405 {"detail": "Method \"GET\" not allowed."}` before a handler runs.
///
/// Registering the action is what makes the guard reachable at all — an
/// unregistered route answers the pipeline's 501 — and the handler below can
/// therefore never execute. It fails loudly if that ever changes.
pub fn guard_only<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Unknown(format!(
            "{} reached a handler its BaseViewSet guard always refuses",
            ctx.view
        )))
    })
}

/// `IntakeSerializer(None).data`.
///
/// When there is no intake, `.first()` returns `None`; `BaseSerializer.data`
/// falls through to `get_initial()` (`rest_framework/serializers.py:512-519`),
/// which emits **every non-read-only field's initial value** — not an empty
/// object. `read_only` fields (`id`, `project_detail`, `pending_issue_count`,
/// `created_at`, `updated_at`, `project`, `workspace`) are skipped by that
/// method, `CharField.initial` is `''`, `BooleanField.initial` is `False` and
/// `JSONField`/`PrimaryKeyRelatedField`/`DateTimeField` default to `None`.
/// Pinned by `GET …/intakes/` on the fixture's `Zeta Empty` project.
#[derive(Debug, serde::Serialize)]
pub struct EmptyIntake {
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub name: &'static str,
    pub description: &'static str,
    pub is_default: bool,
    pub view_props: Option<()>,
    pub logo_props: Option<()>,
    pub created_by: Option<uuid::Uuid>,
    pub updated_by: Option<uuid::Uuid>,
}

/// The body of a missing intake, in `IntakeSerializer.get_initial()` order.
pub fn empty_intake() -> Result<ViewResponse, ViewError> {
    let body = plane_serde::to_vec(
        plane_serde::Dialect::Drf,
        &EmptyIntake {
            deleted_at: None,
            name: "",
            description: "",
            is_default: false,
            view_props: None,
            logo_props: None,
            created_by: None,
            updated_by: None,
        },
    )?;
    Ok(ViewResponse::raw(
        200,
        plane_core::error::APPLICATION_JSON,
        body,
    ))
}
