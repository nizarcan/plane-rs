// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceMemberUserViewsEndpoint.post`
//! (`plane/app/views/workspace/member.py:224-231`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/workspace-views/` (198) | `POST` | [`workspace_views`] |
//!
//! The view is a plain `BaseAPIView` with only `IsAuthenticated`, but its path
//! is selected by the `views` route group (`deploy/route-groups.yaml:184`), so
//! it is registered here — the same arrangement as `project-views` (147),
//! whose handler lives in `projects::settings_write`.
//!
//! ```python
//! workspace_member = WorkspaceMember.objects.get(workspace__slug=slug, member=request.user, is_active=True)
//! workspace_member.view_props = request.data.get("view_props", {})
//! workspace_member.save()
//! return Response(status=204)
//! ```
//!
//! Three behaviours are easy to lose:
//!
//! * `.get(...)` uses the **manager**, so a soft-deleted membership, an inactive
//!   one or a user who is not a workspace member is `DoesNotExist` → the family
//!   404 `{"error": "The required object does not exist."}`. The route has no
//!   role check beyond that: a workspace guest may write their own row.
//! * `request.data.get("view_props", {})` does **not** keep the stored value: an
//!   absent key writes `{}`, a `null` writes SQL `NULL` (refused by the NOT NULL
//!   column → the family 400 `The payload is not valid`), and a form body writes
//!   the JSON *string*.
//! * `save()` is H01 + `auto_now` on the whole row; only `view_props`,
//!   `updated_at` and `updated_by_id` can change, so the UPDATE writes those
//!   three columns (`views::write_queries::WORKSPACE_MEMBER_SAVE_VIEW_PROPS`).

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::write_queries as queries;
use super::write_support::{self, integrity};
use crate::common::{self, jobs};
use crate::write;

/// `WorkspaceMemberUserViewsEndpoint.post` (`workspace/member.py:224-231`).
pub fn workspace_views<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let user = jobs::user_id(ctx)?;

        // `WorkspaceMember.objects.get(...)`: the membership lookup runs before
        // `request.data` is first touched, exactly as in the Python.
        let statement = queries::WORKSPACE_MEMBER_FOR_VIEWS.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&slug, &user])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let member_id: Uuid = row.get(0);

        // `workspace_member.view_props = request.data.get("view_props", {})`
        let view_props = write_support::json_or_empty_object(ctx.data()?, "view_props")?;

        let statement = queries::WORKSPACE_MEMBER_SAVE_VIEW_PROPS
            .prepare(client)
            .await?;
        client
            .execute(
                &statement,
                &[&view_props, &write::now(), &Some(user), &member_id],
            )
            .await
            .map_err(integrity)?;

        Ok(ViewResponse::empty(204))
    })
}
