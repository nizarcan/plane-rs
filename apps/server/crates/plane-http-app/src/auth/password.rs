// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The password routes: 379-382 (forgot/reset, app and space) and 383-384
//! (change/set, shared by both apps).
//!
//! | Route | Python |
//! | --- | --- |
//! | 379 | `views/app/password_management.py:45-96` |
//! | 380 | `views/app/password_management.py:99-175` |
//! | 381 | `views/space/password_management.py:45-108` |
//! | 382 | `views/space/password_management.py:111-159` |
//! | 383 | `views/common.py:48-98` |
//! | 384 | `views/common.py:101-139` |
//!
//! The space reset view is **not** a thinner copy of the app one; three
//! behaviours differ and are kept (`AUTH_FLOWS.md` §5):
//!
//! * the app view catches `(ValueError, User.DoesNotExist)` around the decode
//!   and lookup, the space view catches only `DjangoUnicodeDecodeError`, so a
//!   malformed `uidb64` or an unknown user is a 500 in space;
//! * the app view builds its URLs with `urljoin`, the space view with
//!   `f"{base_host}/accounts/reset-password/?…"`, and `base_host(is_space=True)`
//!   already ends in `/` — the double slash is real;
//! * the app success redirect is `sign-in?success=True`, the space success
//!   redirect is the bare space base.

use chrono::Utc;
use http::Method;
use plane_auth::auth_flow::adapter::AdapterError;
use plane_auth::auth_flow::email::validate_email;
use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::instance;
use plane_auth::auth_flow::password::is_password_strong_async;
use plane_auth::auth_flow::redirect::{urlencode, urljoin};
use plane_auth::auth_flow::reset_token::{
    TokenUser, check_token, make_token, urlsafe_base64_decode, urlsafe_base64_encode,
};
use plane_auth::auth_flow::tasks::view_base_host;
use plane_auth::{LoginFlags, check_password_async, make_password_async};
use plane_core::Object;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::{Message, task_name, tasks};
use plane_serializers::generated::app_user::UserSerializer;
use serde_json::Value;
use uuid::Uuid;

use super::support;
use crate::common::render;
use crate::users_me::write_email::save_user;

/// Register the six password routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.authentication.views.app.password_management.ForgotPasswordEndpoint")?
        .method(Method::POST, app_forgot)?;
    registry
        .django_view("plane.authentication.views.app.password_management.ResetPasswordEndpoint")?
        .method(Method::POST, app_reset)?;
    registry
        .api_view(
            "plane.authentication.views.space.password_management.ForgotPasswordSpaceEndpoint",
        )?
        .method(Method::POST, space_forgot)?;
    registry
        .django_view(
            "plane.authentication.views.space.password_management.ResetPasswordSpaceEndpoint",
        )?
        .method(Method::POST, space_reset)?;
    registry
        .api_view("plane.authentication.views.common.ChangePasswordEndpoint")?
        .method(Method::POST, change_password)?;
    registry
        .api_view("plane.authentication.views.common.SetUserPasswordEndpoint")?
        .method(Method::POST, set_password)?;
    Ok(())
}

fn flags(is_space: bool) -> LoginFlags {
    LoginFlags {
        is_app: !is_space,
        is_space,
        is_admin: false,
    }
}

fn json(error: &AuthFlowError) -> Result<ViewResponse, ViewError> {
    support::json_object(400, &error.get_error_dict())
}

fn adapter_error(error: AdapterError) -> Result<ViewResponse, ViewError> {
    match error {
        AdapterError::Auth(error) => json(&error),
        AdapterError::Internal(message) => Err(ViewError::Unknown(message)),
    }
}

/// 379 `POST /auth/forgot-password/`.
pub fn app_forgot<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { forgot_password(ctx, false).await })
}

/// 381 `POST /auth/spaces/forgot-password/`.
pub fn space_forgot<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { forgot_password(ctx, true).await })
}

/// `ForgotPasswordEndpoint.post` and `ForgotPasswordSpaceEndpoint.post`.
///
/// # Errors
///
/// A database/broker failure (500), or a rendering failure.
async fn forgot_password(ctx: &ViewCtx<'_>, is_space: bool) -> Result<ViewResponse, ViewError> {
    let email = support::data_get(ctx, "email")?;

    let client = ctx.database().await?;
    if let Err(error) = instance::require_setup(client).await {
        return adapter_error(error);
    }
    // The space view reads `(EMAIL_HOST, EMAIL_HOST_USER, EMAIL_HOST_PASSWORD)`
    // in one call but inspects only the host (`space/password_management.py:62-81`);
    // the app view reads the host alone.
    let smtp_configured = if is_space {
        instance::email_configuration(client, ctx.settings)
            .await?
            .smtp_configured()
    } else {
        instance::smtp_configured(client, ctx.settings).await?
    };
    if !smtp_configured {
        return json(&AuthFlowError::new(AuthErrorCode::SmtpNotConfigured));
    }

    let Some(email) = validate_forgot_email(email.as_ref())? else {
        return json(&AuthFlowError::new(AuthErrorCode::InvalidEmail));
    };

    // `User.objects.filter(email=email).first()` — exact, not normalised
    // (`AUTH_FLOWS.md` §5 "Case sensitivity").
    let Some(user) = support::load_user_by_email(client, &email).await? else {
        return json(&AuthFlowError::new(AuthErrorCode::UserDoesNotExist));
    };

    let uidb64 = urlsafe_base64_encode(user.id.to_string().as_bytes());
    let id_string = user.id.to_string();
    let token_user = TokenUser {
        pk: &id_string,
        password: &user.password,
        email: user.email.as_deref(),
        last_login: user.last_login,
    };
    let token = make_token(&token_user, Utc::now(), ctx.settings.secret_key.value());

    let current_site = view_base_host(ctx.settings, flags(is_space))?;
    let message = forgot_password_message(
        user.first_name.clone(),
        user.email.clone().unwrap_or_default(),
        uidb64,
        token,
        current_site,
    );
    crate::common::jobs::dispatch_task(
        ctx,
        task_name::FORGOT_PASSWORD,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await?;

    support::json_object(
        200,
        &Object::new().field("message", "Check your email to reset your password"),
    )
}

/// `validate_email(email)` in the forgot view, where `email` is whatever
/// `request.data.get("email")` returned.
///
/// Python raises `ValidationError` for a falsy value, a list/dict that does not
/// contain the string `"@"`, or an invalid string; a number or bool raises
/// `TypeError` instead (a 500). A dict *containing* `"@"` gets past the
/// membership test and then `str.rsplit` raises `AttributeError` (also a 500).
///
/// # Errors
///
/// [`ViewError::Unknown`] for the two 500 branches.
fn validate_forgot_email(email: Option<&Value>) -> Result<Option<String>, ViewError> {
    match email {
        None | Some(Value::Null) | Some(Value::Array(_)) => Ok(None),
        Some(Value::Object(map)) => {
            if map.contains_key("@") {
                Err(ViewError::Unknown(
                    "AttributeError: 'dict' object has no attribute 'rsplit'".to_string(),
                ))
            } else {
                Ok(None)
            }
        }
        Some(Value::Number(_)) | Some(Value::Bool(_)) => Err(ViewError::Unknown(
            "TypeError: argument of type 'int' is not iterable".to_string(),
        )),
        Some(Value::String(email)) => Ok(Some(email.clone()).filter(|email| validate_email(email))),
    }
}

/// `forgot_password.delay(first_name, email, uidb64, token, current_site)`,
/// including the `current_site=None` case: `base_host`'s view copy returns
/// `None` when no base URL is configured, and Celery records a JSON `null`.
fn forgot_password_message(
    first_name: String,
    email: String,
    uidb64: String,
    token: String,
    current_site: Option<String>,
) -> Message {
    match current_site {
        Some(site) => tasks::forgot_password(first_name, email, uidb64, token, site),
        None => Message::new(
            task_name::FORGOT_PASSWORD,
            vec![
                TaskArg::str(first_name),
                TaskArg::str(email),
                TaskArg::str(uidb64),
                TaskArg::str(token),
                TaskArg::Null,
            ],
            Vec::new(),
        ),
    }
}

/// 380 `POST /auth/reset-password/{uidb64}/{token}/`.
pub fn app_reset<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { reset_password(ctx, false).await })
}

/// 382 `POST /auth/spaces/reset-password/{uidb64}/{token}/`.
pub fn space_reset<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { reset_password(ctx, true).await })
}

/// Why `smart_str(urlsafe_base64_decode(uidb64))` failed.
enum DecodeError {
    /// `ValueError` from `binascii` — caught by the app view, a 500 in space.
    Base64,
    /// `DjangoUnicodeDecodeError` from `smart_str` — caught by both views and
    /// mapped to `EXPIRED_PASSWORD_TOKEN`.
    Unicode,
}

fn decode_uid(uidb64: &str) -> Result<String, DecodeError> {
    let bytes = urlsafe_base64_decode(uidb64).map_err(|_| DecodeError::Base64)?;
    String::from_utf8(bytes).map_err(|_| DecodeError::Unicode)
}

/// `ResetPasswordEndpoint.post` (`app/password_management.py:99-175`) and
/// `ResetPasswordSpaceEndpoint.post` (`space/password_management.py:111-159`).
///
/// # Errors
///
/// The 500 branches Python leaves uncaught (see the module note).
async fn reset_password(ctx: &ViewCtx<'_>, is_space: bool) -> Result<ViewResponse, ViewError> {
    let params = support::post_params(ctx)?;
    let uidb64 = ctx.kwarg("uidb64").unwrap_or_default();
    let token = ctx.kwarg("token").unwrap_or_default();
    let client = ctx.database().await?;
    let flags = flags(is_space);

    // The app view's inner `except (ValueError, User.DoesNotExist)` swallows
    // **both** decode failures: `DjangoUnicodeDecodeError` subclasses
    // `UnicodeDecodeError`, which subclasses `ValueError`, so the outer
    // `except DjangoUnicodeDecodeError` never sees one and its
    // `EXPIRED_PASSWORD_TOKEN` branch is dead there. The space view has no
    // inner catch, so a non-UTF-8 value does reach its outer handler.
    let id = match decode_uid(uidb64) {
        Ok(id) => id,
        Err(DecodeError::Unicode) => {
            if is_space {
                return reset_error(
                    ctx,
                    is_space,
                    &AuthFlowError::new(AuthErrorCode::ExpiredPasswordToken),
                );
            }
            return reset_error(
                ctx,
                is_space,
                &AuthFlowError::new(AuthErrorCode::InvalidPasswordToken),
            );
        }
        Err(DecodeError::Base64) => {
            if is_space {
                return Err(ViewError::Unknown(
                    "ValueError: Invalid base64-encoded string".to_string(),
                ));
            }
            return reset_error(
                ctx,
                is_space,
                &AuthFlowError::new(AuthErrorCode::InvalidPasswordToken),
            );
        }
    };
    // `User.objects.get(id=id)` with a non-UUID string raises `ValidationError`,
    // which neither view catches.
    let user_id = Uuid::parse_str(&id).map_err(|_| ViewError::Validation)?;
    let Some(mut user) = support::load_user_by_id(client, user_id).await? else {
        if is_space {
            // `except DjangoUnicodeDecodeError` only: DoesNotExist is a 500.
            return Err(ViewError::ObjectDoesNotExist);
        }
        return reset_error(
            ctx,
            is_space,
            &AuthFlowError::new(AuthErrorCode::InvalidPasswordToken),
        );
    };

    let id_string = user.id.to_string();
    let email = user.email.clone();
    let password_hash = user.password.clone();
    let token_user = TokenUser {
        pk: &id_string,
        password: &password_hash,
        email: email.as_deref(),
        last_login: user.last_login,
    };
    if !check_token(
        &token_user,
        token,
        Utc::now(),
        ctx.settings.secret_key.value(),
    ) {
        return reset_error(
            ctx,
            is_space,
            &AuthFlowError::new(AuthErrorCode::InvalidPasswordToken),
        );
    }

    let Some(password) = params.get("password").filter(|value| !value.is_empty()) else {
        return reset_error(
            ctx,
            is_space,
            &AuthFlowError::new(AuthErrorCode::InvalidPassword),
        );
    };
    if !is_password_strong_async(password)
        .await
        .map_err(|error| ViewError::Unknown(error.to_string()))?
    {
        return reset_error(
            ctx,
            is_space,
            &AuthFlowError::new(AuthErrorCode::PasswordTooWeak),
        );
    }

    user.password = make_password_async(Some(password.to_string()))
        .await
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    user.is_password_autoset = false;
    save_user(client, user).await?;

    if is_space {
        return support::redirect_url(support::base_host(ctx.settings, flags)?);
    }
    let base = support::base_host(ctx.settings, flags)?;
    let url = urljoin(
        &base,
        &format!(
            "sign-in?{}",
            urlencode(&[("success".to_string(), "True".to_string())])
        ),
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    support::redirect_url(url)
}

/// The reset view's error redirect, app or space.
fn reset_error(
    ctx: &ViewCtx<'_>,
    is_space: bool,
    error: &AuthFlowError,
) -> Result<ViewResponse, ViewError> {
    if is_space {
        let base = support::base_host(ctx.settings, flags(true))?;
        let url = format!(
            "{base}/accounts/reset-password/?{}",
            urlencode(&error.redirect_params())
        );
        return support::redirect_url(url);
    }
    let base = support::base_host(ctx.settings, flags(false))?;
    let target = format!(
        "accounts/reset-password?{}",
        urlencode(&error.redirect_params())
    );
    let url = urljoin(&base, &target).map_err(|error| ViewError::Unknown(error.to_string()))?;
    support::redirect_url(url)
}

/// 383 `POST /auth/change-password/`.
pub fn change_password<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = ctx.database().await?;
        let user_id = ctx.user.id().ok_or(ViewError::Key)?;
        let Some(mut user) = support::load_user_by_id(client, user_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // "If the user password is not autoset then we need to check the old
        // password" (`common.py:54-63`).
        let old_password = support::data_get(ctx, "old_password")?;
        if !user.is_password_autoset && !support::truthy(old_password.as_ref()) {
            return json(
                &AuthFlowError::new(AuthErrorCode::MissingPassword)
                    .field("error", "Old password is missing"),
            );
        }
        let new_password = support::data_get(ctx, "new_password")?;
        if !support::truthy(new_password.as_ref()) {
            return json(
                &AuthFlowError::new(AuthErrorCode::MissingPassword)
                    .field("error", "Old or new password is missing"),
            );
        }
        if !user.is_password_autoset {
            let old = require_string(old_password.as_ref())?;
            let valid = check_password_async(old.to_string(), user.password.clone())
                .await
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
            if !valid {
                return json(
                    &AuthFlowError::new(AuthErrorCode::IncorrectOldPassword)
                        .field("error", "Old password is not correct"),
                );
            }
        }
        let new_password = require_string(new_password.as_ref())?;
        if !is_password_strong_async(new_password)
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?
        {
            return json(&AuthFlowError::new(AuthErrorCode::PasswordTooWeak));
        }

        user.password = make_password_async(Some(new_password.to_string()))
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        user.is_password_autoset = false;
        let mut user = save_user(client, user).await?;
        support::user_login(ctx, client, &mut user, flags(false)).await?;
        support::json_object(
            200,
            &Object::new().field("message", "Password updated successfully"),
        )
    })
}

/// 384 `POST /auth/set-password/`.
///
/// `@invalidate_cache("/api/users/me/")` (`common.py:102`) runs before the view
/// body, exactly like the decorator: the response of a later `GET
/// /api/users/me/` must not carry the pre-change row.
pub fn set_password<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        crate::common::cache::invalidate_cache(ctx, Some("/api/users/me/"), false, false, false)
            .await?;
        let client = ctx.database().await?;
        let user_id = ctx.user.id().ok_or(ViewError::Key)?;
        let Some(mut user) = support::load_user_by_id(client, user_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let password = support::data_get(ctx, "password")?;

        if !user.is_password_autoset {
            return json(
                &AuthFlowError::new(AuthErrorCode::PasswordAlreadySet).field(
                    "error",
                    "Your password is already set please change your password from profile",
                ),
            );
        }
        if !support::truthy(password.as_ref()) {
            return json(&AuthFlowError::new(AuthErrorCode::InvalidPassword));
        }
        let password = require_string(password.as_ref())?;
        if !is_password_strong_async(password)
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?
        {
            return json(&AuthFlowError::new(AuthErrorCode::InvalidPassword));
        }

        user.password = make_password_async(Some(password.to_string()))
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        user.is_password_autoset = false;
        let mut user = save_user(client, user).await?;
        support::user_login(ctx, client, &mut user, flags(false)).await?;

        let serializer = user_serializer(&user);
        let body = render(ctx, &serializer)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `User.check_password`/`is_password_strong` on a non-string raises in
/// Python (`AttributeError`/`TypeError`), which the family handler answers 500.
fn require_string(value: Option<&Value>) -> Result<&str, ViewError> {
    match value {
        Some(Value::String(text)) => Ok(text),
        _ => Err(ViewError::Unknown(
            "TypeError: a password must be a string".to_string(),
        )),
    }
}

/// `UserSerializer(user).data`'s field map, in `to_representation` order.
fn user_serializer(user: &User) -> UserSerializer<'_> {
    UserSerializer {
        last_login: user.last_login,
        id: user.id,
        username: &user.username,
        mobile_number: user.mobile_number.as_deref(),
        email: user.email.as_deref(),
        display_name: &user.display_name,
        first_name: &user.first_name,
        last_name: &user.last_name,
        avatar: &user.avatar,
        avatar_asset: user.avatar_asset_id,
        cover_image: user.cover_image.as_deref(),
        cover_image_asset: user.cover_image_asset_id,
        date_joined: user.date_joined,
        created_at: user.created_at,
        updated_at: user.updated_at,
        last_location: &user.last_location,
        created_location: &user.created_location,
        is_superuser: user.is_superuser,
        is_managed: user.is_managed,
        is_password_expired: user.is_password_expired,
        is_active: user.is_active,
        is_staff: user.is_staff,
        is_email_verified: user.is_email_verified,
        is_password_autoset: user.is_password_autoset,
        is_password_reset_required: user.is_password_reset_required,
        token: &user.token,
        last_active: user.last_active,
        last_login_time: user.last_login_time,
        last_logout_time: user.last_logout_time,
        last_login_ip: &user.last_login_ip,
        last_logout_ip: &user.last_logout_ip,
        last_login_medium: &user.last_login_medium,
        last_login_uagent: &user.last_login_uagent,
        token_updated_at: user.token_updated_at,
        is_bot: user.is_bot,
        bot_type: user.bot_type.as_deref(),
        user_timezone: &user.user_timezone,
        is_email_valid: user.is_email_valid,
        masked_at: user.masked_at,
    }
}
