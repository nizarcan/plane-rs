// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/views/{app,space}/check.py` — routes 377-378
//! `POST /auth/{spaces/}email-check/`.
//!
//! The two files are identical; the response is
//! `{"existing": bool, "status": "MAGIC_CODE" | "CREDENTIAL"}`, where
//! `MAGIC_CODE` needs SMTP configured **and** `ENABLE_MAGIC_LINK_LOGIN == "1"`
//! (an exact string compare, unlike the provider's `== "0"` gate) and, for an
//! existing user, an autoset password.
//!
//! Order matters: the instance gate (400), then the two configuration reads,
//! then the email checks. A missing or `null` email is `EMAIL_REQUIRED`; any
//! other non-string value goes through `str()` first, so `5` is
//! `INVALID_EMAIL`, not `EMAIL_REQUIRED`.

use http::Method;
use plane_auth::auth_flow::adapter::AdapterError;
use plane_auth::auth_flow::email::validate_email;
use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::instance;
use plane_core::Object;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use serde_json::Value;

use super::queries;
use super::support;

/// Register both email-check routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.authentication.views.app.check.EmailCheckEndpoint")?
        .method(Method::POST, app_check)?;
    registry
        .api_view("plane.authentication.views.space.check.EmailCheckSpaceEndpoint")?
        .method(Method::POST, space_check)?;
    Ok(())
}

/// 377 `POST /auth/email-check/`.
pub fn app_check<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { email_check(ctx).await })
}

/// 378 `POST /auth/spaces/email-check/`.
pub fn space_check<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { email_check(ctx).await })
}

/// The body both views run.
///
/// # Errors
///
/// A database failure, or a rendering failure (both 500s in Django).
async fn email_check(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let client = ctx.database().await?;
    if let Err(error) = instance::require_setup(client).await {
        return json_error(error);
    }
    let configuration = instance::email_check_configuration(client, ctx.settings).await?;

    let email = support::data_get(ctx, "email")?;
    if !support::truthy(email.as_ref()) {
        return json_error(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::EmailRequired,
        )));
    }
    // `str(email).lower().strip()` (`check.py:68`).
    let email = support::python_str(email.as_ref().unwrap_or(&Value::Null))
        .to_lowercase()
        .trim()
        .to_string();
    if !validate_email(&email) {
        return json_error(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::InvalidEmail,
        )));
    }

    let statement = queries::AUTH_EMAIL_CHECK_USER.prepare(client).await?;
    let existing_user = client.query_opt(&statement, &[&email]).await?;

    let magic_code = configuration.smtp_configured && configuration.magic_link_login_enabled;
    let (existing, status) = match existing_user {
        Some(row) => {
            let is_password_autoset: bool = row.get(0);
            (
                true,
                if is_password_autoset && magic_code {
                    "MAGIC_CODE"
                } else {
                    "CREDENTIAL"
                },
            )
        }
        None => (
            false,
            if magic_code {
                "MAGIC_CODE"
            } else {
                "CREDENTIAL"
            },
        ),
    };
    support::json_object(
        200,
        &Object::new()
            .field("existing", Value::Bool(existing))
            .field("status", status),
    )
}

/// `Response(exc.get_error_dict(), status=400)`.
fn json_error(error: AdapterError) -> Result<ViewResponse, ViewError> {
    match error {
        AdapterError::Auth(error) => support::json_object(400, &error.get_error_dict()),
        AdapterError::Internal(message) => Err(ViewError::Unknown(message)),
    }
}
