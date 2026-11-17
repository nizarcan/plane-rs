// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Routes 359-364: the magic-link flow (`plans/AUTH_FLOWS.md` §3, set B).
//!
//! | # | Route | Python |
//! | --- | --- | --- |
//! | 359 | `POST /auth/magic-generate/` | `views/app/magic.py:36-61` |
//! | 360 | `POST /auth/magic-sign-in/` | `views/app/magic.py:64-133` |
//! | 361 | `POST /auth/magic-sign-up/` | `views/app/magic.py:136-201` |
//! | 362 | `POST /auth/spaces/magic-generate/` | `views/space/magic.py:35-59` |
//! | 363 | `POST /auth/spaces/magic-sign-in/` | `views/space/magic.py:62-119` |
//! | 364 | `POST /auth/spaces/magic-sign-up/` | `views/space/magic.py:122-178` |
//!
//! The Redis state machine is `plane_auth::auth_flow::MagicCodeProvider` (P0b);
//! the provider chain is `complete_login_or_signup` (P0a). This module is the
//! view layer: the instance gate, the body, the redirects and the login side
//! effects.
//!
//! ## Behaviour that looks wrong and is ported as-is
//!
//! * **An invalid or empty e-mail is a 500 on 359/362.**
//!   `django.core.validators.validate_email` (`views/app/magic.py:53`) raises
//!   `ValidationError`, which the view's `except AuthenticationException` does
//!   not catch and DRF's `exception_handler` does not map: the client gets
//!   Django's 500 page. `/auth/email-check/` answers `INVALID_EMAIL` for the
//!   same input. `{"email": null}` is
//!   `AttributeError: 'NoneType' object has no attribute 'strip'` (`:51`),
//!   also a 500.
//! * The verify path builds the provider with `key=f"magic_{email}"` (`:102`),
//!   so `SMTP_NOT_CONFIGURED`/`MAGIC_LINK_LOGIN_DISABLED` carry
//!   `magic_<email>` in their payload, while the generate path carries the bare
//!   address (`magic_code.py:71,78`).
//! * `get_redirection_path` returns bare slugs (`redirection_path.py:14,30,46`),
//!   which `validate_next_path` drops for lack of a leading `/`: an app
//!   sign-in/up without `next_path` lands on the base URL, never a workspace.
//! * The space success redirect is `base_host(...).rstrip("/") +
//!   validate_next_path(next_path)` (`space/magic.py:106`), so no `next_path`
//!   yields the base **without** its trailing slash.
//! * `login()` rotates the CSRF secret (`django/contrib/auth/__init__.py:217`),
//!   so every successful sign-in/up answers with a fresh `csrftoken` cookie.
//! * `@throttle_auth_redirect` (`rate_limit.py:62-95`) runs
//!   `AuthenticationThrottle` **before** the handler reads the body; the
//!   rejection is a 302 carrying `RATE_LIMIT_EXCEEDED`, not a 429. `plane-drf`
//!   records the decorator in `routes.json` but does not apply it to plain
//!   views, so this module runs it explicitly (see
//!   [`crate::auth::support::throttle_redirect`]).
//! * `login()` assigns `request.user`, so Django's request log names the new
//!   user. A handler cannot write `RequestContext::user` (the dispatcher owns
//!   it), so the log line keeps `user_id` empty on these routes. It is the one
//!   observable difference and it is log-only.
//! * `MagicGenerateEndpoint`'s `try` also wraps the broker publish (`:57`), but
//!   `except AuthenticationException` cannot catch an AMQP failure: a broker
//!   outage is a 500 on both sides.

use deadpool_postgres::Client;
use http::Method;
use plane_auth::LoginFlags;
use plane_auth::auth_flow::{
    AdapterContext, AdapterError, AuthCallback, AuthErrorCode, AuthFlowError, MagicCodeFlags,
    MagicCodeProvider, MagicLinkConfiguration, PostUserAuthWorkflow, complete_login_or_signup,
    magic_link, magic_link_configuration, profile_for_user, require_setup, signup_enabled,
    validate_email,
};
use plane_core::error::APPLICATION_JSON;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::{Message, task_name};
use plane_jobs::queue::Job;
use plane_serde::pyjson::Value as PyValue;
use plane_serde::{Dialect, to_vec};

use crate::auth::support;
use crate::common::jobs;

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register routes 359-364.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.authentication.views.app.magic.MagicGenerateEndpoint")?
        .method(Method::POST, generate)?;
    registry
        .api_view("plane.authentication.views.space.magic.MagicGenerateSpaceEndpoint")?
        .method(Method::POST, generate_space)?;
    registry
        .django_view("plane.authentication.views.app.magic.MagicSignInEndpoint")?
        .method(Method::POST, sign_in)?;
    registry
        .django_view("plane.authentication.views.app.magic.MagicSignUpEndpoint")?
        .method(Method::POST, sign_up)?;
    registry
        .django_view("plane.authentication.views.space.magic.MagicSignInSpaceEndpoint")?
        .method(Method::POST, space_sign_in)?;
    registry
        .django_view("plane.authentication.views.space.magic.MagicSignUpSpaceEndpoint")?
        .method(Method::POST, space_sign_up)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 359 / 362: MagicGenerateEndpoint.post
// ---------------------------------------------------------------------------

/// `MagicGenerateEndpoint.post` (`views/app/magic.py:41-61`).
fn generate<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { generate_impl(ctx).await })
}

/// `MagicGenerateSpaceEndpoint.post` (`views/space/magic.py:40-59`): the same
/// logic, written out independently in Python.
fn generate_space<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { generate_impl(ctx).await })
}

async fn generate_impl(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let client = ctx.database().await?;
    // `instance = Instance.objects.first()` then `instance is None or not
    // instance.is_setup_done` (`:43-49`).
    if let Err(error) = require_setup(client).await {
        return auth_error_response(error);
    }

    // `email = request.data.get("email", "").strip().lower()` (`:51`).
    let email = email_field(ctx.data()?)?;

    // `validate_email(email)` sits outside the `except AuthenticationException`
    // (`:52-53`), so a malformed address is Django's 500 page.
    if !validate_email(&email) {
        return Err(ViewError::Unknown(
            "ValidationError: ['Enter a valid email address.']".to_string(),
        ));
    }

    let redis = redis(ctx)?;
    let configuration = magic_link_configuration(client, ctx.settings)
        .await
        .map_err(ViewError::from)?;
    // `MagicCodeProvider(request=request, key=email)`; both configuration
    // rejections are inside the `try` (`:54-59`).
    let provider = match MagicCodeProvider::new(email.clone(), None, magic_flags(&configuration)) {
        Ok(provider) => provider,
        Err(error) => return auth_error_response(error),
    };
    let issued = match provider.initiate(client, redis).await {
        Ok(issued) => issued,
        Err(error) => return auth_error_response(error),
    };

    // `magic_link.delay(email, key, token)` (`:57`): outside the `try`, so a
    // broker failure is a 500.
    let message = magic_link(email, issued.key.clone(), issued.token.clone());
    publish(ctx, &message).await?;

    // `Response({"key": str(key)}, status=200)`; one key, so insertion order
    // cannot matter.
    Ok(ViewResponse::json(
        200,
        serde_json::json!({ "key": issued.key }),
    ))
}

// ---------------------------------------------------------------------------
// 360 / 361 / 363 / 364: the plain redirect views
// ---------------------------------------------------------------------------

/// The four plain views, as the differences between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    AppSignIn,
    AppSignUp,
    SpaceSignIn,
    SpaceSignUp,
}

impl Flow {
    fn is_signup(self) -> bool {
        matches!(self, Self::AppSignUp | Self::SpaceSignUp)
    }

    fn is_app(self) -> bool {
        matches!(self, Self::AppSignIn | Self::AppSignUp)
    }

    fn is_space(self) -> bool {
        !self.is_app()
    }

    /// `MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED` / `…_SIGN_UP_…` (`:73-76`, `:145-148`,
    /// `space/magic.py:71-74`, `:131-134`).
    fn missing_code_error(self) -> AuthErrorCode {
        if self.is_signup() {
            AuthErrorCode::MagicSignUpEmailCodeRequired
        } else {
            AuthErrorCode::MagicSignInEmailCodeRequired
        }
    }

    fn login_flags(self) -> LoginFlags {
        LoginFlags {
            is_app: self.is_app(),
            is_space: self.is_space(),
            is_admin: false,
        }
    }
}

/// `MagicSignInEndpoint.post` (`views/app/magic.py:66-133`).
fn sign_in<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { redirect_flow_impl(ctx, Flow::AppSignIn).await })
}

/// `MagicSignUpEndpoint.post` (`views/app/magic.py:138-201`).
fn sign_up<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { redirect_flow_impl(ctx, Flow::AppSignUp).await })
}

/// `MagicSignInSpaceEndpoint.post` (`views/space/magic.py:64-119`).
fn space_sign_in<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { redirect_flow_impl(ctx, Flow::SpaceSignIn).await })
}

/// `MagicSignUpSpaceEndpoint.post` (`views/space/magic.py:124-178`).
fn space_sign_up<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { redirect_flow_impl(ctx, Flow::SpaceSignUp).await })
}

async fn redirect_flow_impl(ctx: &ViewCtx<'_>, flow: Flow) -> Result<ViewResponse, ViewError> {
    // `request.POST`: layer 5's CSRF check already parsed the same body.
    let post = support::post_params(ctx)?;
    // `request.POST.get("code", "").strip()`,
    // `request.POST.get("email", "").strip().lower()` and
    // `request.POST.get("next_path")` (`:68-70`, `:140-142`, …).
    let code = python_strip(post.get_or("code", "")).to_string();
    let email = python_strip(post.get_or("email", "")).to_lowercase();
    let next_path = post.get("next_path").map(str::to_string);
    let flags = flow.login_flags();

    // `@throttle_auth_redirect(...)` wraps the handler (`rate_limit.py:77-93`),
    // so it runs before any of the reads below.
    if let Some(response) = support::throttle_redirect(ctx, next_path.as_deref(), flags).await? {
        return Ok(response);
    }

    if code.is_empty() || email.is_empty() {
        return support::redirect_error(
            ctx,
            flags,
            next_path.as_deref(),
            &AuthFlowError::new(flow.missing_code_error()),
        );
    }

    // `existing_user = User.objects.filter(email=email).first()` — exact, and
    // outside the `try`.
    let client = ctx.database().await?;
    let exists = user_exists(client, &email).await?;
    if !flow.is_signup() && !exists {
        return support::redirect_error(
            ctx,
            flags,
            next_path.as_deref(),
            &AuthFlowError::new(AuthErrorCode::UserDoesNotExist),
        );
    }
    if flow.is_signup() && exists {
        return support::redirect_error(
            ctx,
            flags,
            next_path.as_deref(),
            &AuthFlowError::new(AuthErrorCode::UserAlreadyExist),
        );
    }

    // The `try:` block (`:101-133`, `:171-200`, `:99-119`, `:158-177`).
    let mut user = match authenticate(ctx, client, flow, &email, &code).await {
        Ok(user) => user,
        Err(AdapterError::Auth(error)) => {
            return support::redirect_error(ctx, flags, next_path.as_deref(), &error);
        }
        Err(AdapterError::Internal(message)) => return Err(ViewError::Unknown(message)),
    };

    // App sign-in reads the profile for the `is_password_autoset and
    // profile.is_onboarded` branch *before* `user_login` (`:109-112`).
    let profile = if flow == Flow::AppSignIn {
        Some(
            profile_for_user(client, &user)
                .await
                .map_err(support::internal_error)?,
        )
    } else {
        None
    };

    // `user_login(...)`: the session and CSRF handles are the middleware's, so
    // `login()`'s `rotate_token` makes layer 5 write the one `csrftoken` cookie
    // (`django/contrib/auth/__init__.py:217`, `csrf.py:505-517`), appending it
    // before the session layer's cookie exactly as Django's phases 5 then 3 do.
    support::user_login(ctx, client, &mut user, flags).await?;

    if flow.is_app() {
        // `path = str(next_path) if next_path else str(get_redirection_path(user))`
        // (`:117`, `:182-185`), except for the autoset+onboarded branch.
        let path = if let Some(profile) = profile
            && user.is_password_autoset
            && profile.is_onboarded
        {
            "/".to_string()
        } else {
            match next_path.as_deref().filter(|value| !value.is_empty()) {
                Some(next) => next.to_string(),
                None => support::redirection_path(client, &user).await?,
            }
        };
        return support::app_success_redirect(ctx, flags, Some(&path));
    }
    // `next_path = validate_next_path(next_path)`;
    // `url = f"{base_host(...).rstrip('/')}{next_path}"` and the
    // `url_has_allowed_host_and_scheme` fallback (`space/magic.py:105-110`).
    support::space_success_redirect(ctx, flags, next_path.as_deref())
}

// ---------------------------------------------------------------------------
// The provider chain
// ---------------------------------------------------------------------------

/// `provider.authenticate()` (`CredentialAdapter.authenticate`,
/// `adapter/credential.py:18-20`): `set_user_data()` then
/// `complete_login_or_signup()`.
async fn authenticate(
    ctx: &ViewCtx<'_>,
    client: &Client,
    flow: Flow,
    email: &str,
    code: &str,
) -> Result<User, AdapterError> {
    let configuration = magic_link_configuration(client, ctx.settings).await?;
    let mut provider = MagicCodeProvider::new(
        format!("magic_{email}"),
        Some(code.to_string()),
        magic_flags(&configuration),
    )?;
    let redis = redis_opt(ctx).ok_or_else(|| {
        AdapterError::Internal(
            "the process has no Redis connection; redis_instance() would raise".to_string(),
        )
    })?;
    provider.set_user_data(client, redis).await?;

    // `Adapter.__check_signup` reads `ENABLE_SIGNUP` only on the signup branch
    // (`base.py:99-117`); `complete_login_or_signup` takes the resolved boolean,
    // so it is resolved eagerly here — one extra configuration read on a plain
    // sign-in, no query in the signup case that Django would skip.
    let enable_signup = signup_enabled(client, ctx.settings).await?;

    let context = AdapterContext {
        client,
        settings: ctx.settings,
        ctx: ctx.ctx,
        publisher: ctx.celery(),
        cache: ctx.redis,
    };
    // Only the app views pass `post_user_auth_workflow` (`magic.py:106`,
    // `:176`); the four `/spaces/` views never do (`space/magic.py:100,159`).
    let callback: Option<&dyn AuthCallback> = if flow.is_app() {
        Some(&PostUserAuthWorkflow)
    } else {
        None
    };
    let completed = complete_login_or_signup(&context, &provider, callback, enable_signup).await?;
    Ok(completed.user)
}

// ---------------------------------------------------------------------------
// Request parsing
// ---------------------------------------------------------------------------

/// `request.data.get("email", "").strip().lower()` on 359/362 (`:51`).
///
/// `request.data` must be a dict or a `QueryDict`; a JSON scalar/list has no
/// `.get`, and a JSON member that is not a string has no `.strip`. Both are the
/// `AttributeError` that becomes Django's 500 page.
fn email_field(data: &Data) -> Result<String, ViewError> {
    match data {
        Data::Form(query) => Ok(python_strip(query.get_or("email", "")).to_lowercase()),
        Data::Json(PyValue::Dict(members)) => match members.get("email") {
            None => Ok(String::new()),
            Some(PyValue::Str(text)) => {
                Ok(python_strip(text.as_str().unwrap_or_default()).to_lowercase())
            }
            Some(other) => Err(attribute_error(other, "strip")),
        },
        Data::Json(other) => Err(ViewError::Unknown(format!(
            "AttributeError: '{}' object has no attribute 'get'",
            python_type_name(other)
        ))),
    }
}

/// `AttributeError: '<type>' object has no attribute '<attribute>'`.
fn attribute_error(value: &PyValue, attribute: &str) -> ViewError {
    ViewError::Unknown(format!(
        "AttributeError: '{}' object has no attribute '{attribute}'",
        python_type_name(value)
    ))
}

/// Python's class name for the value, as `AttributeError` spells it.
fn python_type_name(value: &PyValue) -> &'static str {
    match value {
        PyValue::Null => "NoneType",
        PyValue::Bool(_) => "bool",
        PyValue::Int(_) => "int",
        PyValue::Float(_) => "float",
        PyValue::Str(_) => "str",
        PyValue::List(_) => "list",
        PyValue::Dict(_) => "dict",
    }
}

/// Python's `str.strip()`: `str.isspace()` also returns true for U+001C..U+001F,
/// which `char::is_whitespace` (the Unicode `White_Space` property) does not.
fn python_strip(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
    })
}

// ---------------------------------------------------------------------------
// Small adapters
// ---------------------------------------------------------------------------

/// `MagicCodeProvider.__init__`'s resolved flags (`magic_code.py:57-79`).
fn magic_flags(configuration: &MagicLinkConfiguration) -> MagicCodeFlags {
    MagicCodeFlags {
        smtp_configured: configuration.smtp_configured(),
        magic_link_login_enabled: !configuration.login_disabled(),
    }
}

/// `User.objects.filter(email=email).first()` (`:86`, `:157`) — only
/// truthiness is read.
async fn user_exists(client: &Client, email: &str) -> Result<bool, ViewError> {
    let statement = plane_auth::queries::USER_EXISTS_BY_EMAIL
        .prepare(client)
        .await?;
    let row = client.query_one(&statement, &[&email]).await?;
    Ok(row.get(0))
}

/// `magic_link.delay(...)` through the ownership router (Celery today).
async fn publish(ctx: &ViewCtx<'_>, message: &Message) -> Result<(), ViewError> {
    let task = task_name::MAGIC_LINK;
    jobs::dispatch_task(ctx, task, message, Job::new(task)).await?;
    Ok(())
}

/// The DRF view's `except AuthenticationException` branch: 400 with
/// `get_error_dict()` in insertion order, so it is rendered by the DRF dialect
/// rather than through a `serde_json::Value` (whose object order is sorted).
fn auth_error_response(error: AdapterError) -> Result<ViewResponse, ViewError> {
    match error {
        AdapterError::Auth(error) => Ok(ViewResponse::raw(
            400,
            APPLICATION_JSON,
            to_vec(Dialect::Drf, &error.get_error_dict())?,
        )),
        AdapterError::Internal(message) => Err(ViewError::Unknown(message)),
    }
}

/// `redis_instance()` (`plane/settings/redis.py:8-16`): the raw, unprefixed
/// Redis client the magic keys live in.
fn redis<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a plane_cache::Redis, ViewError> {
    redis_opt(ctx).ok_or_else(|| {
        ViewError::Unknown(
            "the process has no Redis connection; redis_instance() would raise".to_string(),
        )
    })
}

fn redis_opt<'a>(ctx: &'a ViewCtx<'a>) -> Option<&'a plane_cache::Redis> {
    ctx.redis
}
