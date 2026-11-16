// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/module/issue.py` — `ModuleIssueViewSet.list`
//! (`GET …/modules/{module_id}/issues/`).
//!
//! The list body is the grouped/plain issue page of `plane-query`; the scope,
//! the `created_at` ordering default (`module/issue.py:113`) and the fact that
//! this view passes the filtered queryset to `issue_group_values`
//! (`module/issue.py:151`) live in [`crate::cycles::issue_list`].

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::cycles::base::{params, uuid_kwarg};
use crate::cycles::issue_list::{self, Request, ScopeKind};

/// `ModuleIssueViewSet.list` (`module/issue.py:95-208`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let module_id: Uuid = uuid_kwarg(ctx, "module_id")?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        issue_list::run(
            ctx,
            Request {
                params,
                scope: ScopeKind::Module(module_id),
                default_order: "created_at",
                group_values_issues: true,
                timezone,
                query: ctx.query(),
            },
        )
        .await
    })
}
