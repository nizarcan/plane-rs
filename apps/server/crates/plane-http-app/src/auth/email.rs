// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/views/{app,space}/email.py` — password sign-in and
//! sign-up (routes 352-355).
//!
//! The two variants share every branch except five behaviours (`AUTH_FLOWS.md`
//! §5): the redirect base, the space success redirect that appends the next
//! path instead of using `get_safe_redirect_url`, the missing
//! `post_user_auth_workflow` callback (so no invite is joined), the absence of
//! `get_redirection_path`, and the space suffix of the error codes' payloads.
//!
//! The view's order is the Python's, including the queries: instance gate →
//! required fields → email validation → the exact `users.email` lookup → the
//! `EmailProvider` constructor's `ENABLE_EMAIL_PASSWORD` gate → the provider's
//! own reload/password check → `complete_login_or_signup` (which reloads the
//! row a third time) → `user_login` → redirect.

use http::Method;
use plane_auth::LoginFlags;
use plane_auth::auth_flow::adapter::AdapterError;
use plane_auth::auth_flow::email::validate_email;
use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::instance;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::support;

/// Register the four password-flow routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .django_view("plane.authentication.views.app.email.SignInAuthEndpoint")?
        .method(Method::POST, app_sign_in)?;
    registry
        .django_view("plane.authentication.views.app.email.SignUpAuthEndpoint")?
        .method(Method::POST, app_sign_up)?;
    registry
        .django_view("plane.authentication.views.space.email.SignInAuthSpaceEndpoint")?
        .method(Method::POST, space_sign_in)?;
    registry
        .django_view("plane.authentication.views.space.email.SignUpAuthSpaceEndpoint")?
        .method(Method::POST, space_sign_up)?;
    Ok(())
}

/// 352 `POST /auth/sign-in/`.
pub fn app_sign_in<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { sign_in(ctx, false).await })
}

/// 353 `POST /auth/sign-up/`.
pub fn app_sign_up<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { sign_up(ctx, false).await })
}

/// 354 `POST /auth/spaces/sign-in/`.
pub fn space_sign_in<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { sign_in(ctx, true).await })
}

/// 355 `POST /auth/spaces/sign-up/`.
pub fn space_sign_up<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { sign_up(ctx, true).await })
}

fn flags(is_space: bool) -> LoginFlags {
    LoginFlags {
        is_app: !is_space,
        is_space,
        is_admin: false,
    }
}

/// `AuthenticationException.get_error_dict()` or the 500 Python would raise.
fn adapter_error(
    ctx: &ViewCtx<'_>,
    flags: LoginFlags,
    next_path: Option<&str>,
    error: AdapterError,
) -> Result<ViewResponse, ViewError> {
    match error {
        AdapterError::Auth(error) => support::redirect_error(ctx, flags, next_path, &error),
        AdapterError::Internal(message) => Err(ViewError::Unknown(message)),
    }
}

/// `SignInAuthEndpoint.post` (`views/app/email.py:32-137`) and
/// `SignInAuthSpaceEndpoint.post` (`views/space/email.py:28-109`).
async fn sign_in(ctx: &ViewCtx<'_>, is_space: bool) -> Result<ViewResponse, ViewError> {
    let flags = flags(is_space);
    let params = support::post_params(ctx)?;
    let next_path = params.get("next_path").map(str::to_owned);

    if let Some(response) = support::throttle_redirect(ctx, next_path.as_deref(), flags).await? {
        return Ok(response);
    }

    let client = ctx.database().await?;
    if let Err(error) = instance::require_setup(client).await {
        return adapter_error(ctx, flags, next_path.as_deref(), error);
    }

    let email = params.get("email");
    let password = params.get("password");
    if !email.is_some_and(|value| !value.is_empty())
        || !password.is_some_and(|value| !value.is_empty())
    {
        // `payload={"email": str(email)}`: a missing key is `str(False)` =
        // `"False"`, an empty one stays `""`.
        let payload_email = email.map_or_else(|| "False".to_string(), str::to_owned);
        let error =
            AuthFlowError::with_email(AuthErrorCode::RequiredEmailPasswordSignIn, payload_email);
        return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
    }
    let email = email.unwrap_or_default().trim().to_lowercase();
    if !validate_email(&email) {
        let error = AuthFlowError::with_email(AuthErrorCode::InvalidEmailSignIn, &email);
        return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
    }
    if support::load_user_by_email(client, &email).await?.is_none() {
        let error = AuthFlowError::with_email(AuthErrorCode::UserDoesNotExist, &email);
        return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
    }

    let completed = match support::email_authenticate(
        ctx,
        client,
        &email,
        password.unwrap_or_default(),
        false,
        is_space,
    )
    .await
    {
        Ok(completed) => completed,
        Err(error) => return adapter_error(ctx, flags, next_path.as_deref(), error),
    };
    let mut completed = completed;
    support::user_login(ctx, client, &mut completed.user, flags).await?;

    if is_space {
        // `views/space/email.py:98-103`: the *request's* next path, not a
        // redirection-path fallback.
        return support::space_success_redirect(ctx, flags, next_path.as_deref());
    }
    let path = match next_path.as_deref() {
        Some(path) if !path.is_empty() => path.to_string(),
        _ => support::redirection_path(client, &completed.user).await?,
    };
    support::app_success_redirect(ctx, flags, Some(&path))
}

/// `SignUpAuthEndpoint.post` (`views/app/email.py:141-244`) and
/// `SignUpAuthSpaceEndpoint.post` (`views/space/email.py:113-194`).
async fn sign_up(ctx: &ViewCtx<'_>, is_space: bool) -> Result<ViewResponse, ViewError> {
    let flags = flags(is_space);
    let params = support::post_params(ctx)?;
    let next_path = params.get("next_path").map(str::to_owned);

    if let Some(response) = support::throttle_redirect(ctx, next_path.as_deref(), flags).await? {
        return Ok(response);
    }

    let client = ctx.database().await?;
    if let Err(error) = instance::require_setup(client).await {
        return adapter_error(ctx, flags, next_path.as_deref(), error);
    }

    let email = params.get("email");
    let password = params.get("password");
    if !email.is_some_and(|value| !value.is_empty())
        || !password.is_some_and(|value| !value.is_empty())
    {
        let payload_email = email.map_or_else(|| "False".to_string(), str::to_owned);
        let error =
            AuthFlowError::with_email(AuthErrorCode::RequiredEmailPasswordSignUp, payload_email);
        return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
    }
    let email = email.unwrap_or_default().trim().to_lowercase();
    if !validate_email(&email) {
        let error = AuthFlowError::with_email(AuthErrorCode::InvalidEmailSignUp, &email);
        return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
    }
    if support::load_user_by_email(client, &email).await?.is_some() {
        let error = AuthFlowError::with_email(AuthErrorCode::UserAlreadyExist, &email);
        return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
    }

    let completed = match support::email_authenticate(
        ctx,
        client,
        &email,
        password.unwrap_or_default(),
        true,
        is_space,
    )
    .await
    {
        Ok(completed) => completed,
        Err(error) => return adapter_error(ctx, flags, next_path.as_deref(), error),
    };
    let mut completed = completed;
    support::user_login(ctx, client, &mut completed.user, flags).await?;

    if is_space {
        return support::space_success_redirect(ctx, flags, next_path.as_deref());
    }
    let path = match next_path.as_deref() {
        Some(path) if !path.is_empty() => path.to_string(),
        _ => support::redirection_path(client, &completed.user).await?,
    };
    support::app_success_redirect(ctx, flags, Some(&path))
}
