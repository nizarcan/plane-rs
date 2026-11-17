// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/views/{app,space}/signout.py` — routes 356-357.
//!
//! `SignOutAuthEndpoint` (`app/signout.py:16-28`) ignores `next_path` and
//! redirects to the app base; `SignOutAuthSpaceEndpoint`
//! (`space/signout.py:17-33`) runs it through `get_safe_redirect_url`.
//!
//! Both wrap the whole write in `try/except Exception`: a missing
//! `REMOTE_ADDR` makes `last_logout_ip` NULL, the save raises `IntegrityError`,
//! and the view still answers its redirect **without** calling `logout()` (the
//! call is inside the `try`). Sign-out is idempotent for a second reason:
//! `request.user.id` is `None` for `AnonymousUser`, so `User.objects.get(pk=None)`
//! raises before anything else.

use http::Method;
use plane_auth::LoginFlags;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::support;

/// Register both sign-out routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .django_view("plane.authentication.views.app.signout.SignOutAuthEndpoint")?
        .method(Method::POST, app_sign_out)?;
    registry
        .django_view("plane.authentication.views.space.signout.SignOutAuthSpaceEndpoint")?
        .method(Method::POST, space_sign_out)?;
    Ok(())
}

fn flags(is_space: bool) -> LoginFlags {
    LoginFlags {
        is_app: !is_space,
        is_space,
        is_admin: false,
    }
}

/// 356 `POST /auth/sign-out/`.
pub fn app_sign_out<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { sign_out(ctx, false).await })
}

/// 357 `POST /auth/spaces/sign-out/`.
pub fn space_sign_out<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { sign_out(ctx, true).await })
}

/// The body of both sign-out views.
///
/// # Errors
///
/// Only the redirect's own failures: everything inside the Python `try` is
/// swallowed, and a malformed `next_path`/missing base URL is a 500 because
/// those calls sit outside it.
async fn sign_out(ctx: &ViewCtx<'_>, is_space: bool) -> Result<ViewResponse, ViewError> {
    let flags = flags(is_space);
    let params = support::post_params(ctx)?;
    let next_path = params.get("next_path").map(str::to_owned);

    if let Some(user_id) = current_user_id(ctx).await {
        let _ = record_logout(ctx, user_id).await;
    }

    if is_space {
        let base = support::base_host(ctx.settings, flags)?;
        let hosts = plane_auth::auth_flow::redirect::settings_allowed_hosts(ctx.settings)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let url = plane_auth::auth_flow::redirect::get_safe_redirect_url(
            &base,
            next_path.as_deref(),
            &[],
            &hosts,
        )
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
        return Ok(support::redirect_url(url));
    }
    let base = support::base_host(ctx.settings, flags)?;
    Ok(support::redirect_url(base))
}

/// `request.user.id`, with the lazy evaluation's failure swallowed like
/// `signout.py`'s `except Exception`.
async fn current_user_id(ctx: &ViewCtx<'_>) -> Option<uuid::Uuid> {
    match ctx.request_user().await {
        Ok(user) => user.id(),
        Err(_) => None,
    }
}

/// The `try` block: load the row, stamp `last_logout_ip`/`last_logout_time`,
/// `save()`, then `logout()`.
///
/// Every failure (no client IP, a database error, a pool failure) leaves the
/// session alone, exactly like the swallowed exception.
async fn record_logout(ctx: &ViewCtx<'_>, user_id: uuid::Uuid) -> Result<(), ViewError> {
    let client = ctx.database().await?;
    let Some(mut user) = support::load_user_by_id(client, user_id).await? else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    user.last_logout_ip = ctx.ctx.client_ip().ok_or(ViewError::Integrity)?;
    user.last_logout_time = Some(crate::write::now());
    crate::users_me::write_email::save_user(client, user).await?;
    ctx.session().flush(client).await?;
    Ok(())
}
