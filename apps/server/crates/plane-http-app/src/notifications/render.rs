// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Rendering and the shared refusal used by the `notifications` handlers.
//!
//! `NotificationSerializer` and `UserNotificationPreferenceSerializer` are
//! plain `ModelSerializer`s, so DRF never passes `fields=`/`expand=` to them
//! (`rest_framework/serializers.py:465-485`); the context below has no mask for
//! that reason, and the nested `UserLiteSerializer` inherits the same one.

use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;

/// The active timezone, or `settings.TIME_ZONE` (UTC).
pub fn timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// The context a plain serializer renders under.
pub fn plain_context<'a>(ctx: &'a ViewCtx<'a>) -> RenderCtx<'a> {
    let timezone: &'a Tz = match ctx.timezone {
        Some(timezone) => timezone,
        None => &chrono_tz::UTC,
    };
    RenderCtx::new(timezone, None, &[])
}

/// A handler for an action the runtime's `BaseViewSet` guard always refuses
/// (`plane/app/views/base.py:103-198`). Registering the action is what makes
/// the guard reachable — an unregistered route answers the pipeline's 501 —
/// and the handler can therefore never execute.
pub fn guard_only<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Unknown(format!(
            "{} reached a handler its BaseViewSet guard always refuses",
            ctx.view
        )))
    })
}
