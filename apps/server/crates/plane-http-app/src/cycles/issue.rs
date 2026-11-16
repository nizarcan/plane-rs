// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/cycle/issue.py` — `CycleIssueViewSet.list`
//! (`GET …/cycles/{cycle_id}/cycle-issues/`).
//!
//! The list body is the grouped/plain issue page of `plane-query`; the scope,
//! the ordering default (`-created_at`) and the `issue_group_values` call shape
//! live in [`super::issue_list`].

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::cycles::base::{params, uuid_kwarg};
use crate::cycles::issue_list::{self, Request, ScopeKind};

/// `CycleIssueViewSet.list` (`cycle/issue.py:108-221`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id: Uuid = uuid_kwarg(ctx, "cycle_id")?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        issue_list::run(
            ctx,
            Request {
                params,
                scope: ScopeKind::Cycle(cycle_id),
                default_order: "-created_at",
                group_values_issues: false,
                timezone,
                query: ctx.query(),
            },
        )
        .await
    })
}
