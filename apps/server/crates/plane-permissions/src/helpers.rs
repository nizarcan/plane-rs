// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `issue_hidden_from_guest` (`plane/app/permissions/base.py:106-141`).
//!
//! Not a permission class: the issue sub-resource views call it from
//! `initial()`/the handler and translate `true` into their own 403 — the issue
//! views raise `PermissionDenied("You are not allowed to view this issue")`
//! (a `{"detail": …}` body), the attachment and comment views return
//! `{"error": "You are not allowed to view this issue"}`. This crate returns
//! the boolean only; the caller owns the body, exactly as in Python.
//!
//! A restricted guest is an active project member with `role=GUEST` on a
//! project whose `guest_view_all_features` is false. Such a caller is hidden
//! from an issue they did not create; ownership uses `Issue.objects` (the
//! soft-deletion manager), so the guest's own archived, draft and triage
//! issues stay visible.

use crate::queries as catalog;
use crate::{Error, PermCtx, parse_uuid};

/// Return `true` when the caller must not see the issue or its sub-resources.
pub async fn issue_hidden_from_guest(
    ctx: &PermCtx<'_>,
    slug: &str,
    project_id: &str,
    issue_id: &str,
) -> Result<bool, Error> {
    // `member=request.user` in the first filter: an anonymous caller raises the
    // same ValidationError here as inside a permission class.
    let user = ctx.user_id()?;
    let project_id = parse_uuid("project_id", project_id)?;

    let statement = catalog::RESTRICTED_GUEST_EXISTS.prepare(ctx.client).await?;
    let row = ctx
        .client
        .query_one(&statement, &[&user, &project_id, &slug])
        .await?;
    let is_restricted_guest: bool = row.get(0);
    if !is_restricted_guest {
        return Ok(false);
    }

    let issue_id = parse_uuid("issue_id", issue_id)?;
    let statement = catalog::ISSUE_OWNED_BY_USER.prepare(ctx.client).await?;
    let row = ctx
        .client
        .query_one(&statement, &[&issue_id, &project_id, &user, &slug])
        .await?;
    let owns_issue: bool = row.get(0);
    Ok(!owns_issue)
}
