// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `DELETE /api/users/me/` — `UserEndpoint.deactivate`
//! (`plane/app/views/user/base.py:278-374`).
//!
//! The view is a sequence of autocommitting statements, and every write before
//! the last one survives a later failure (`ATOMIC_REQUESTS` is unset):
//!
//! 1. `InstanceAdmin.objects.filter(user=user).exists()` → the 400 guard
//!    (`:283-287`);
//! 2. `ProjectMember.objects.filter(member=request.user, is_active=True)` with
//!    two annotations, iterated; `bulk_update(..., ["is_active"])` (`:292-334`);
//! 3. the same pair over `WorkspaceMember` (`:313-336`);
//! 4. `WorkspaceMemberInvite.objects.filter(email=user.email).delete()` — H04,
//!    a soft delete (`:339`);
//! 5. `Session.objects.filter(user_id=request.user.id).delete()` — **every**
//!    session row of the user, including the one serving the request (`:342`);
//! 6. `Profile.objects.get(user=user)` then the reset and a full save
//!    (`:345-357`);
//! 7. the password reset and the user's deactivation, then a full `save()`
//!    (`:359-367`, H30);
//! 8. `user_deactivation_email.delay(base_host(...), user.id)` (`:370`);
//! 9. `logout(request)` (`:373`) — `request.session.flush()`, whose response
//!    half deletes the `session-id` cookie and adds `Vary: Cookie`;
//! 10. `204`.
//!
//! ## The two "only admin" guards are dead code
//!
//! `plans/USERS_ME.md` §4: the queryset is already `member=request.user`, so
//! `~Q(member=request.user)` can never be true and `Count(Case(..., default=0))`
//! counts one non-NULL zero per row group; `Count("id")` over a primary-key
//! grouped page is also 1. `other_admin_exists > 0` is therefore always true
//! and the 400 branches at `:308-311,329-332` are unreachable. They are ported
//! as written (scan, then flip every row) so the SQL and the row set stay
//! Django's; the scenario proves the deactivation proceeds.
//!
//! ## Sessions
//!
//! Step 5 removes the current row before `logout()`, which then deletes it
//! again (a no-op) and clears the store. The response therefore carries
//! `Set-Cookie: session-id=""; Max-Age=0` and the caller's next request is 401.

use plane_db::JsonText;
use plane_db::models::user::Profile;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::task_name;
use plane_jobs::queue::Job;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use super::queries::USER_PROFILE;
use super::user_base;
use super::write_email;
use super::write_queries;
use crate::common::{client, jobs};
use crate::write;

/// `UserEndpoint.deactivate` (`plane/app/views/user/base.py:278-374`).
pub fn deactivate<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `get_object()` is `self.request.user` (`:69-70`): no query. The row
        // is the one the session loader already read.
        let mut user = user_base::user_row(ctx).await?.into_owned();
        let client = client(ctx)?;

        // 1. `InstanceAdmin.objects.filter(user=user).exists()` (`:283`).
        let statement = write_queries::INSTANCE_ADMIN_EXISTS.prepare(client).await?;
        if client.query_opt(&statement, &[&user.id]).await?.is_some() {
            return Ok(write::error_body(
                400,
                "You cannot deactivate your account since you are an instance admin",
            ));
        }

        // 2. Project memberships (`:292-334`). The annotated loop never takes
        //    its error branch (see the module docs), so the row set is exactly
        //    the scan's.
        let project_ids =
            scan_memberships(client, &write_queries::PROJECT_MEMBER_SCAN, user.id).await?;
        apply_membership_deactivation(
            client,
            &write_queries::PROJECT_MEMBER_DEACTIVATE,
            &project_ids,
        )
        .await?;

        // 3. Workspace memberships (`:313-336`).
        let workspace_ids =
            scan_memberships(client, &write_queries::WORKSPACE_MEMBER_SCAN, user.id).await?;
        apply_membership_deactivation(
            client,
            &write_queries::WORKSPACE_MEMBER_DEACTIVATE,
            &workspace_ids,
        )
        .await?;

        // 4. `WorkspaceMemberInvite.objects.filter(email=user.email).delete()`
        //    (`:339`). H04: `deleted_at = now`, no `updated_at`, no cascade.
        let now = write::now();
        let statement = write_queries::INVITE_DELETE_BY_EMAIL
            .prepare(client)
            .await?;
        client.execute(&statement, &[&user.email, &now]).await?;

        // 5. `Session.objects.filter(user_id=request.user.id).delete()`
        //    (`:342`). The column is `varchar(50)`, so the bind is `str(uuid)`.
        plane_auth::delete_all_for_user(client, user.id).await?;

        // 6. `Profile.objects.get(user=user)` (`:345`); a missing row is
        //    `DoesNotExist`, which the family handler renders as its 404.
        let statement = USER_PROFILE.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&user.id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let mut profile = Profile::from_row(&row, 0)?;
        profile.last_workspace_id = None;
        profile.is_tour_completed = false;
        profile.is_onboarded = false;
        // The literal dict, in source order, through Django's `json.dumps`.
        profile.onboarding_step = JsonText::from_json(&onboarding_step())?;
        profile.updated_at = write::now();
        let statement = write_queries::PROFILE_UPDATE.prepare(client).await?;
        client.execute(&statement, &profile_binds(&profile)).await?;

        // 7. The password reset (`:359-367`). `set_password(uuid.uuid4().hex)`
        //    hashes a random plaintext on `spawn_blocking`, like Django's
        //    `PBKDF2PasswordHasher.encode`.
        user.is_password_autoset = true;
        user.password =
            plane_auth::make_password_async(Some(Uuid::new_v4().simple().to_string())).await?;
        user.is_active = false;
        // `user_ip(request)` (`plane/authentication/utils/host.py:88-89`).
        // `CharField` is NOT NULL, so Python's `None` assignment would make the
        // UPDATE an `IntegrityError` — the family 400.
        user.last_logout_ip = ctx.ctx.client_ip().ok_or(ViewError::Integrity)?;
        user.last_logout_time = Some(write::now());
        let user = write_email::save_user(client, user).await?;

        // 8. `user_deactivation_email.delay(base_host(request, is_app=True),
        //    user.id)` (`:370`): `(str, uuid.UUID)`.
        let origin = plane_auth::base_host(
            ctx.settings,
            plane_auth::LoginFlags {
                is_app: true,
                ..Default::default()
            },
        )?;
        let task = task_name::USER_DEACTIVATION_EMAIL;
        let message = plane_jobs::celery::tasks::user_deactivation_email(origin, user.id);
        jobs::dispatch_task(ctx, task, &message, Job::new(task)).await?;

        // 9. `logout(request)` (`:373`): flush; the runtime applies the
        //    AnonymousUser half and the middleware the cookie half.
        ctx.session().flush(client).await?;

        // 10. `Response(status=204)`.
        Ok(ViewResponse::empty(204))
    })
}

/// One membership scan: `SELECT id … ORDER BY created_at DESC`.
async fn scan_memberships(
    client: &deadpool_postgres::Client,
    query: &plane_db::Query,
    user_id: Uuid,
) -> Result<Vec<Uuid>, ViewError> {
    let statement = query.prepare(client).await?;
    let rows = client.query(&statement, &[&user_id]).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        ids.push(row.get(0));
    }
    Ok(ids)
}

/// `bulk_update(rows, ["is_active"], batch_size=100)` (`:334,336`).
///
/// Only the named column is written; `save()` never runs, so no `auto_now`
/// timestamp or H01 audit column moves. An empty list writes nothing, as in
/// Django.
async fn apply_membership_deactivation(
    client: &deadpool_postgres::Client,
    query: &plane_db::Query,
    ids: &[Uuid],
) -> Result<(), ViewError> {
    if ids.is_empty() {
        return Ok(());
    }
    let statement = query.prepare(client).await?;
    for id in ids {
        client.execute(&statement, &[id]).await?;
    }
    Ok(())
}

/// `profile.onboarding_step = {"workspace_join": False, …}` (`:351-356`), in
/// the dict's source order.
fn onboarding_step() -> Json {
    let mut step = PyDict::new();
    step.insert(PyStr::new("workspace_join"), Json::Bool(false));
    step.insert(PyStr::new("profile_complete"), Json::Bool(false));
    step.insert(PyStr::new("workspace_create"), Json::Bool(false));
    step.insert(PyStr::new("workspace_invite"), Json::Bool(false));
    Json::Dict(step)
}

/// The bind vector of [`write_queries::PROFILE_UPDATE`]: `USER_PROFILE`'s
/// columns without `id`, then `id`.
///
/// Shared with set A's 167/171/172 (`write_profile`), which write the same
/// full row.
pub(super) fn profile_binds(profile: &Profile) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    vec![
        &profile.created_at,
        &profile.updated_at,
        &profile.user_id,
        &profile.theme,
        &profile.is_app_rail_docked,
        &profile.is_tour_completed,
        &profile.onboarding_step,
        &profile.use_case,
        &profile.role,
        &profile.is_onboarded,
        &profile.last_workspace_id,
        &profile.billing_address_country,
        &profile.billing_address,
        &profile.has_billing_address,
        &profile.company_name,
        &profile.notification_view_mode,
        &profile.is_smooth_cursor_enabled,
        &profile.is_mobile_onboarded,
        &profile.mobile_onboarding_step,
        &profile.mobile_timezone_auto_set,
        &profile.language,
        &profile.start_of_the_week,
        &profile.goals,
        &profile.background_color,
        &profile.is_navigation_tour_completed,
        &profile.has_marketing_email_consent,
        &profile.is_subscribed_to_changelog,
        &profile.product_tour,
        &profile.id,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_step_is_the_python_literal_in_source_order() {
        let json = JsonText::from_json(&onboarding_step()).expect("json.dumps");
        assert_eq!(
            json.as_str(),
            r#"{"workspace_join": false, "profile_complete": false, "workspace_create": false, "workspace_invite": false}"#
        );
    }
}
