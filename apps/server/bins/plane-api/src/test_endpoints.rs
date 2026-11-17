// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Test-only endpoints, compiled only with `--features test-endpoints`.
//!
//! They exist to prove the Phase 2 exit check ("a session created by either
//! backend works on the other") before the real `/auth/` routes are ported in
//! Phase 6. They are **never** routed by the proxy: `/_plane_rs/` is outside
//! `/api/`, `/auth/` and `/static/`, which are the only prefixes
//! `apps/proxy/Caddyfile.ce` sends to the API, so production traffic cannot
//! reach them; and without the feature the routes do not exist at all.
//!
//! | Route | Purpose |
//! | --- | --- |
//! | `GET /_plane_rs/test/whoami` | resolve `request.user` through the normal lazy pipeline and report the id |
//! | `POST /_plane_rs/test/login` | verify a password and perform Django's `login()` + Plane's `user_login()` |
//! | `GET /_plane_rs/test/csrf-token` | `django.middleware.csrf.get_token`, i.e. `/auth/get-csrf-token/` |
//!
//! The CSRF token endpoint is what lets the interop script mint a token with
//! Rust and have Django accept it; it is the same `get_token` the real
//! `/auth/get-csrf-token/` view calls.
//!
//! All three go through the normal pipeline, so they see the same session
//! layer, user extraction, CSRF check and response headers as a real route.

use axum::http::Method;
use plane_auth::{AuthError, LoginFlags, RequestUser, user_login};
use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;

use crate::app::AppState;
use crate::middleware::auth::{AuthContext, RequestDb};
use crate::response::Resp;

/// Which test endpoint the path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRoute {
    Whoami,
    Login,
    CsrfToken,
}

/// Resolve `/_plane_rs/test/...` to an endpoint and method.
pub fn route(path: &str, method: Method) -> Option<TestRoute> {
    let route = match path {
        "/_plane_rs/test/whoami" => TestRoute::Whoami,
        "/_plane_rs/test/login" => TestRoute::Login,
        "/_plane_rs/test/csrf-token" => TestRoute::CsrfToken,
        _ => return None,
    };
    let allowed = match route {
        TestRoute::Whoami | TestRoute::CsrfToken => method == Method::GET,
        TestRoute::Login => method == Method::POST,
    };
    allowed.then_some(route)
}

/// Run the endpoint. Errors become Django's 500 page.
pub async fn handle(
    state: &AppState,
    ctx: &mut plane_core::context::RequestContext,
    auth: &mut AuthContext,
    db: &mut RequestDb,
    route: TestRoute,
) -> Resp {
    let result = match route {
        TestRoute::Whoami => whoami(state, ctx, auth, db).await,
        TestRoute::Login => login(state, ctx, auth, db).await,
        TestRoute::CsrfToken => Ok(csrf_token(auth).await),
    };
    match result {
        Ok(resp) => resp,
        Err(error) => {
            tracing::error!(target: "plane.api", %error, "test endpoint failed");
            crate::handlers::server_error()
        }
    }
}

/// `{"user_id": "…" | null, "session_key_present": true | false}`.
async fn whoami(
    state: &AppState,
    ctx: &mut plane_core::context::RequestContext,
    auth: &mut AuthContext,
    db: &mut RequestDb,
) -> Result<Resp, AuthError> {
    auth.resolve(state, ctx, db).await?;
    let cookie_name = crate::middleware::auth::cookie_name(&state.settings, &ctx.path);
    let body = Object::new()
        .field(
            "user_id",
            match auth.user.id() {
                Some(id) => serde_json::Value::String(id.to_string()),
                None => serde_json::Value::Null,
            },
        )
        .field("session_key_present", ctx.cookie(cookie_name).is_some());
    json(200, &body)
}

/// `POST {"email": …, "password": …}`.
///
/// `SignInAuthEndpoint` lower-cases and strips the email before looking it up
/// (`plane/authentication/views/app/email.py:57`); this does the same, then
/// `ModelBackend.authenticate` (which also rehashes an outdated hash), then
/// `login()` + `user_login(is_app=True)`.
async fn login(
    state: &AppState,
    ctx: &mut plane_core::context::RequestContext,
    auth: &mut AuthContext,
    db: &mut RequestDb,
) -> Result<Resp, AuthError> {
    let Some(body) = ctx.body.as_deref() else {
        return Ok(detail(400, "A JSON body is required."));
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Ok(detail(400, "A JSON body is required."));
    };
    let Some(email) = value.get("email").and_then(|value| value.as_str()) else {
        return Ok(detail(400, "email is required."));
    };
    let Some(password) = value.get("password").and_then(|value| value.as_str()) else {
        return Ok(detail(400, "password is required."));
    };
    let email = email.trim().to_lowercase();

    let client = db.client(state).await?;
    let Some(user) = plane_auth::user::authenticate_password(client, &email, password).await?
    else {
        return Ok(detail(401, "Invalid credentials."));
    };

    // Refresh the user the session resolves to: `login()` sets `request.user`.
    auth.set_user(RequestUser::from_user(user.clone()), ctx)
        .await;
    {
        let mut session = auth.session.lock().await;
        let mut csrf = auth.csrf.lock().await;
        user_login(
            client,
            &state.settings,
            ctx,
            &mut session,
            &mut csrf,
            &user,
            LoginFlags {
                is_app: true,
                is_admin: false,
                is_space: false,
            },
        )
        .await?;
    }
    auth.set_user(RequestUser::from_user(user.clone()), ctx)
        .await;
    let body = Object::new().field("user_id", user.id.to_string());
    json(200, &body)
}

/// `django.middleware.csrf.get_token` behind the same JSON shape as
/// `/auth/get-csrf-token/`.
async fn csrf_token(auth: &AuthContext) -> Resp {
    let token = auth.csrf.lock().await.get_token();
    match json(200, &Object::new().field("csrf_token", token)) {
        Ok(resp) => resp,
        Err(_) => crate::handlers::server_error(),
    }
}

fn json(status: u16, body: &Object) -> Result<Resp, AuthError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Drf, body)
        .map_err(|error| AuthError::Database(format!("failed to render JSON: {error}")))?;
    Ok(Resp::new(status, APPLICATION_JSON, bytes))
}

fn detail(status: u16, message: &str) -> Resp {
    match json(status, &Object::new().field("detail", message)) {
        Ok(resp) => resp,
        Err(_) => crate::handlers::server_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_methods() {
        assert_eq!(
            route("/_plane_rs/test/whoami", Method::GET),
            Some(TestRoute::Whoami)
        );
        assert_eq!(
            route("/_plane_rs/test/login", Method::POST),
            Some(TestRoute::Login)
        );
        assert_eq!(
            route("/_plane_rs/test/csrf-token", Method::GET),
            Some(TestRoute::CsrfToken)
        );
        assert_eq!(route("/_plane_rs/test/whoami", Method::POST), None);
        assert_eq!(route("/_plane_rs/test/login", Method::GET), None);
        assert_eq!(route("/_plane_rs/test/unknown", Method::GET), None);
        assert_eq!(route("/api/users/me/", Method::GET), None);
    }
}
