// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `AccountEndpoint.delete` (`plane/app/views/user/base.py:436-439`), set A of
//! the `users-me` writes.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 169 `DELETE /api/users/me/accounts/{pk}/` | `delete` | `user/base.py:436-439` | [`delete`] |
//! | 168 `DELETE /api/users/me/accounts/` | — | the `pk` `TypeError` | [`delete`] |
//!
//! ## The 168 signature 500
//!
//! One class serves both URL shapes (`plane/app/urls/user.py:49-50`), and
//! `routes.json` maps `DELETE` onto each. `delete(self, request, pk)` requires
//! the kwarg, so the collection route makes DRF's
//! `handler(request, *args, **kwargs)` raise
//! `TypeError: AccountEndpoint.delete() missing 1 required positional argument:
//! 'pk'` (`rest_framework/views.py:517-523`) — uncaught, so Plane's family
//! handler answers `500 {"error": "Something went wrong please try again
//! later"}` (`plane/app/views/base.py:330-334`) after `IsAuthenticated`.
//!
//! ## The delete is a hard DELETE
//!
//! `Account` is a `TimeAuditModel`, not a `SoftDeleteModel`
//! (`plane/db/models/user.py:271`): `account.delete()` is Django's deletion
//! collector, one real `DELETE` of the row, with no `deleted_at` column, no
//! `updated_by`, no `auto_now` and no `soft_delete_related_objects` task. The
//! lookup's miss is a `DoesNotExist` → the family 404
//! `{"error": "The required object does not exist."}` (`views/base.py:318-322`),
//! not DRF's `{"detail": …}`.
//!
//! The body is never parsed: a malformed `DELETE` body still deletes.

use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::queries;
use super::support;
use super::write_a_queries;
use crate::common::client;

/// `AccountEndpoint.delete` (`plane/app/views/user/base.py:436-439`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // 168 has no `pk` kwarg: the call itself raises before the body is
        // touched.
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Unknown(
                "TypeError: AccountEndpoint.delete() missing 1 required positional argument: 'pk'"
                    .to_string(),
            ));
        };
        let user_id = support::user_id(ctx)?;
        let client = client(ctx)?;
        // The route's `<uuid:pk>` converter already matched, so a parse failure
        // cannot happen; Django would answer 404 before the view.
        let pk = crate::common::lookup::uuid_kwarg("pk", pk)?;

        // `Account.objects.get(pk=pk, user=request.user)` (`:437`): the
        // `accounts` table has no soft-delete manager, so this is the row or a
        // miss. Before the body is parsed.
        let statement = queries::USER_ACCOUNT_BY_ID.prepare(client).await?;
        if client
            .query_opt(&statement, &[&pk, &user_id])
            .await?
            .is_none()
        {
            return Err(ViewError::ObjectDoesNotExist);
        }

        // `account.delete()` (`:438`).
        let statement = write_a_queries::ACCOUNT_DELETE.prepare(client).await?;
        client.execute(&statement, &[&pk]).await?;

        // `Response(status=204)`.
        Ok(ViewResponse::empty(204))
    })
}
