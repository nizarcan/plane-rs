// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IntakeIssueViewSet.destroy` — set D of `plans/INTAKE.md` §3 (routes 56,
//! 60).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `DELETE …/intake-issues/{pk}/` (56) | [`destroy`] | `plane/app/views/intake/base.py:554-571` |
//! | `DELETE …/inbox-issues/{pk}/` (60) | [`destroy`] | the same method |
//! | `PATCH` on either | set C | `:335-505` |
//!
//! `{pk}` is the **Issue** id, not the link id: the view resolves the
//! `IntakeIssue` row by `issue_id=pk`.
//!
//! ## Order, as Python runs it
//!
//! 1. `@allow_permission(allowed_roles=[ROLE.ADMIN], creator=True,
//!    model=Issue)` (`:554`, `plane/app/permissions/base.py:19-103`). The
//!    runtime evaluates it before this handler (`plane-drf::dispatch`):
//!    * `WorkspaceMember` active on the URL slug, else 403;
//!    * `ProjectMember` active on the URL project, else 403;
//!    * `Issue.objects.filter(id=pk, created_by=request.user).exists()` —
//!      **unscoped** by workspace or project, and it returns before the role
//!      check, so a MEMBER (or GUEST) who created the issue may delete it;
//!    * otherwise `ProjectMember.role == ADMIN`, or a project member who is a
//!      workspace admin, else 403.
//! 2. `Intake.objects.filter(workspace__slug, project_id).first()`
//!    (`:556`) — `super::queries::INTAKE_ONLY`, ordered by `name`.
//! 3. `IntakeIssue.objects.get(issue_id=pk, workspace__slug, project_id,
//!    intake_id=intake_id)` (`:557-562`):
//!    * passing the `Intake` instance (or `None`) as `intake_id` normalises to
//!      its pk (`django/db/models/fields/related_lookups.py:15-40`), so a
//!      project with **no** intake binds `NULL`, matches nothing and raises
//!      `DoesNotExist` → 404 — not the 500 `retrieve`'s `intake_id.id` gives;
//!    * the manager excludes soft-deleted links, so a soft-deleted
//!      `IntakeIssue` is also 404.
//! 4. `if intake_issue.status in [-2, -1, 0, 2]:` — every status **except**
//!    accepted (`1`) also deletes the issue:
//!    `Issue.objects.filter(workspace__slug, project_id, pk).first()`
//!    (`:567`) through the soft-deletion manager, so a live link whose issue
//!    is soft-deleted yields `None` and `None.delete()` raises `AttributeError`
//!    → 500 **with nothing written**. A found issue is `issue.delete()`: H03, a
//!    full `Issue.save()` — H10a (`_ensure_default_state`), H10b
//!    (`completed_at` when the state was just assigned), H12 (`strip_tags`),
//!    H01 (`updated_by`) — then `soft_delete_related_objects.delay("db",
//!    "issue", pk)` (`plane/db/models/issue.py:180-224`,
//!    `plane/db/mixins.py:72-78`).
//! 5. `intake_issue.delete()`: the same H03 for the link, cascading
//!    `soft_delete_related_objects.delay("db", "intakeissue", pk)`
//!    (`plane/db/models/intake.py:54-110`; H16's `snoozed_till` floor is
//!    skipped because `has_changed("snoozed_till")` is false on delete).
//! 6. `204` with no body.
//!
//! The view opens no `transaction.atomic()`, so each step commits on its own
//! and the 500 in step 4 leaves the link (and everything before it) in place.

use plane_db::hooks;
use plane_db::models::intake::IntakeIssue;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::queries;
use super::write_intake_issue_delete_queries as queries_write;
use super::write_queries;
use crate::issues::lists::list_common::{client, kwarg_text, kwarg_uuid, user_id};
use crate::issues::write_issue::{ensure_state_for_save, strip_html};
use crate::write;

/// Register set D's handler on `IntakeIssueViewSet`.
///
/// `list`/`retrieve` are registered by [`super::issue::register`]; `create`
/// (set B) and `partial_update` (set C) add their own actions.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.intake.base.IntakeIssueViewSet")?
        .action("destroy", destroy)?;
    Ok(())
}

/// `IntakeIssueViewSet.destroy` (`plane/app/views/intake/base.py:554-571`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `intake_id = Intake.objects.filter(...).first()` (`:556`).
        let intake_statement = client
            .prepare_cached(queries::INTAKE_ONLY.sql)
            .await
            .map_err(ViewError::from)?;
        let intake_id: Option<Uuid> = client
            .query_opt(&intake_statement, &[&slug, &project_id])
            .await
            .map_err(ViewError::from)?
            .map(|row| row.get(0));

        // `IntakeIssue.objects.get(issue_id=pk, …, intake_id=intake_id)`
        // (`:557-562`). A missing intake binds SQL NULL (`intake_id = NULL` is
        // never true), which is this lookup's 404.
        let link_statement = client
            .prepare_cached(write_queries::INTAKE_ISSUE_FOR_ISSUE.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&link_statement, &[&intake_id, &pk, &project_id, &slug])
            .await
            .map_err(ViewError::from)?;
        let intake_issue = match rows.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            [row] => IntakeIssue::from_row(row, 0)?,
            // `QuerySet.get()` evaluates every row and raises
            // `MultipleObjectsReturned`, which is not an `ObjectDoesNotExist`.
            _ => {
                return Err(ViewError::Unknown(
                    "IntakeIssue.MultipleObjectsReturned: the get() matched more than one row"
                        .to_owned(),
                ));
            }
        };

        // `if intake_issue.status in [-2, -1, 0, 2]:` (`:565`).
        if matches!(intake_issue.status, -2 | -1 | 0 | 2) {
            // `Issue.objects.filter(...).first()` (`:567`): the default
            // soft-deletion manager hides a soft-deleted issue.
            let issue_statement = client
                .prepare_cached(queries_write::ISSUE_FOR_INTAKE_DELETE.sql)
                .await
                .map_err(ViewError::from)?;
            let Some(row) = client
                .query_opt(&issue_statement, &[&slug, &project_id, &pk])
                .await
                .map_err(ViewError::from)?
            else {
                // `issue.delete()` on `None`.
                return Err(ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'delete'".to_owned(),
                ));
            };
            let mut issue = Issue::from_row(&row, 0)?;

            // `issue.delete()` → H03 (`plane/db/mixins.py:72-78`): deleted_at,
            // a full `Issue.save()` (H10a/H10b/H12/H01) and the cascade task.
            // Same sequence as `IssueViewSet.destroy`
            // (`plane/app/views/issue/base.py:718-741`).
            let state_changed = ensure_state_for_save(client, &mut issue).await?;
            let now = write::now();
            issue.deleted_at = Some(now);
            issue.description_stripped = strip_html(&issue.description_html);
            hooks::issue::update_issue_row(client, &mut issue, state_changed, Some(user), now)
                .await?;
            write::publish_soft_delete_cascade(ctx, "db", "issue", pk).await?;
        }

        // `intake_issue.delete()` → H03 for the link.
        write::soft_delete(
            client,
            "intake_issues",
            "id",
            intake_issue.id,
            Some(user),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "intakeissue", intake_issue.id).await?;

        Ok(ViewResponse::empty(204))
    })
}
