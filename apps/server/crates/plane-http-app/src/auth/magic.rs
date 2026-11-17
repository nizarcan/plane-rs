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
//!   views, so this module runs it explicitly (see [`decorator_throttle`]).
//! * `login()` assigns `request.user`, so Django's request log names the new
//!   user. A handler cannot write `RequestContext::user` (the dispatcher owns
//!   it), so the log line keeps `user_id` empty on these routes. It is the one
//!   observable difference and it is log-only.
//! * `MagicGenerateEndpoint`'s `try` also wraps the broker publish (`:57`), but
//!   `except AuthenticationException` cannot catch an AMQP failure: a broker
//!   outage is a 500 on both sides.

use deadpool_postgres::Client;
use http::Method;
use plane_auth::auth_flow::{
    AdapterContext, AdapterError, AuthCallback, AuthErrorCode, AuthFlowError, MagicCodeFlags,
    MagicCodeProvider, MagicLinkConfiguration, PostUserAuthWorkflow, RedirectionPath,
    complete_login_or_signup, get_safe_redirect_url, magic_link, magic_link_configuration,
    profile_for_user, require_setup, settings_allowed_hosts, signup_enabled, space_redirect_url,
    validate_email, view_base_host,
};
use plane_auth::{AuthUser, CsrfState, LoginFlags, user_login};
use plane_core::error::{APPLICATION_JSON, TEXT_HTML};
use plane_core::py;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::parsers::{self, Data};
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::request::QueryDict;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::{Message, task_name};
use plane_jobs::queue::Job;
use plane_serde::pyjson::Value as PyValue;
use plane_serde::{Dialect, to_vec};
use plane_throttle::{StaticUser, ThrottleClass, ThrottleOutcome, ThrottleRequest};

use crate::auth::queries;
use crate::common::jobs;

/// `HttpResponseRedirect`'s content type (`django/http/response.py:190-205`):
/// status 302, an empty body, `text/html; charset=utf-8`.
const HTML_UTF8: &str = TEXT_HTML;

/// `HttpResponseRedirect.DEFAULT_STATUS` (`django/http/response.py:190`).
const REDIRECT_STATUS: u16 = 302;

/// The DRF parser classes `request.POST` corresponds to: Django parses only the
/// two form media types (`django/http/request.py:335-359`).
const FORM_PARSERS: [&str; 2] = [
    "rest_framework.parsers.MultiPartParser",
    "rest_framework.parsers.FormParser",
];

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
    let post = request_post(ctx)?;
    // `request.POST.get("code", "").strip()`,
    // `request.POST.get("email", "").strip().lower()` and
    // `request.POST.get("next_path")` (`:68-70`, `:140-142`, …).
    let code = python_strip(post.get_or("code", "")).to_string();
    let email = python_strip(post.get_or("email", "")).to_lowercase();
    let next_path = post.get("next_path").map(str::to_string);

    // `@throttle_auth_redirect(...)` wraps the handler (`rate_limit.py:77-93`),
    // so it runs before any of the reads below.
    if let Some(response) = decorator_throttle(ctx, flow, next_path.as_deref()).await? {
        return Ok(response);
    }

    if code.is_empty() || email.is_empty() {
        return error_redirect(
            ctx,
            flow,
            next_path.as_deref(),
            &AuthFlowError::new(flow.missing_code_error()),
        );
    }

    // `existing_user = User.objects.filter(email=email).first()` — exact, and
    // outside the `try`.
    let client = ctx.database().await?;
    let exists = user_exists(client, &email).await?;
    if !flow.is_signup() && !exists {
        return error_redirect(
            ctx,
            flow,
            next_path.as_deref(),
            &AuthFlowError::new(AuthErrorCode::UserDoesNotExist),
        );
    }
    if flow.is_signup() && exists {
        return error_redirect(
            ctx,
            flow,
            next_path.as_deref(),
            &AuthFlowError::new(AuthErrorCode::UserAlreadyExist),
        );
    }

    // The `try:` block (`:101-133`, `:171-200`, `:99-119`, `:158-177`).
    let user = match authenticate(ctx, client, flow, &email, &code).await {
        Ok(user) => user,
        Err(AdapterError::Auth(error)) => {
            return error_redirect(ctx, flow, next_path.as_deref(), &error);
        }
        Err(AdapterError::Internal(message)) => return Err(ViewError::Unknown(message)),
    };

    // App sign-in reads the profile for the `is_password_autoset and
    // profile.is_onboarded` branch *before* `user_login` (`:109-112`).
    let profile = if flow == Flow::AppSignIn {
        Some(
            profile_for_user(client, &user)
                .await
                .map_err(internal_error)?,
        )
    } else {
        None
    };

    let csrf_cookie = login(ctx, client, flow, &user).await?;

    let url = if flow.is_app() {
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
                None => redirection_path(client, &user).await?,
            }
        };
        let base = flow_base_host(ctx, flow)?;
        safe_redirect(ctx, &base, Some(&path), &[])?
    } else {
        // `next_path = validate_next_path(next_path)`;
        // `url = f"{base_host(...).rstrip('/')}{next_path}"` and the
        // `url_has_allowed_host_and_scheme` fallback (`space/magic.py:105-110`).
        let base = flow_base_host(ctx, flow)?;
        let hosts = settings_allowed_hosts(ctx.settings).map_err(|error| py_value_error(&error))?;
        space_redirect_url(&base, next_path.as_deref(), &hosts)
            .map_err(|error| py_value_error(&error))?
    };

    // `login()` rotated the CSRF secret, so layer 5 writes a new `csrftoken`
    // cookie (`django/contrib/auth/__init__.py:217`, `csrf.py:505-517`).
    let mut response = redirect_response(&url);
    if let Some(cookie) = csrf_cookie {
        response = response.with_header("Set-Cookie", cookie);
    }
    Ok(response)
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

/// `get_redirection_path(user)` (`authentication/utils/redirection_path.py:8-47`).
///
/// Every variant is a **bare** value; the caller passes it through
/// `validate_next_path`, which drops it. The string is what the Python `path`
/// variable holds, so the caller's behaviour matches even though the redirect
/// does not carry it.
async fn redirection_path(client: &Client, user: &User) -> Result<String, ViewError> {
    // `profile, _ = Profile.objects.get_or_create(user=user)` (`:11`).
    let profile = profile_for_user(client, user)
        .await
        .map_err(internal_error)?;
    if !profile.is_onboarded {
        return Ok(RedirectionPath::Onboarding.slug().to_string());
    }
    if let Some(workspace_id) = profile.last_workspace_id {
        // `.exists()` then `.first()` on the same queryset (`:17-32`); one
        // `LIMIT 1` answers both.
        let statement = queries::AUTH_LAST_WORKSPACE_SLUG.prepare(client).await?;
        if let Some(row) = client
            .query_opt(&statement, &[&workspace_id, &user.id])
            .await?
        {
            return Ok(row.get(0));
        }
    }
    let statement = queries::AUTH_MEMBER_WORKSPACE_SLUG.prepare(client).await?;
    if let Some(row) = client.query_opt(&statement, &[&user.id]).await? {
        return Ok(row.get(0));
    }
    let email = user.email.clone().unwrap_or_default();
    let statement = queries::AUTH_PENDING_INVITE_EXISTS.prepare(client).await?;
    let row = client.query_one(&statement, &[&email]).await?;
    if row.get::<_, bool>(0) {
        return Ok(RedirectionPath::Invitations.slug().to_string());
    }
    Ok(RedirectionPath::CreateWorkspace.slug().to_string())
}

/// `plane/authentication/utils/login.py:13-27` `user_login(request, user, …)`.
///
/// Returns the CSRF cookie the login rotation asked layer 5 to write; the
/// handler appends it so the response's header order is `Location`,
/// `Set-Cookie: csrftoken`, `Set-Cookie: session-id` — the order Django's
/// response phases 5 then 3 produce.
async fn login(
    ctx: &ViewCtx<'_>,
    client: &Client,
    flow: Flow,
    user: &User,
) -> Result<Option<String>, ViewError> {
    let auth_user = auth_user(user);
    let mut csrf = CsrfState::default();
    // `login()` writes `request.user = user`. `ViewCtx::ctx` is shared, so the
    // write goes to a clone whose only reader is `user_login`'s own
    // `device_info`/`base_host` (see the module note about the log line).
    let mut request = ctx.ctx.clone();
    let mut session = ctx.session().lock().await;
    user_login(
        client,
        ctx.settings,
        &mut request,
        &mut session,
        &mut csrf,
        &auth_user,
        flow.login_flags(),
    )
    .await?;
    drop(session);
    Ok(csrf.response_cookie(ctx.settings))
}

/// The `users` row as the session layer needs it (`AuthUser`, `user.rs:56-77`).
fn auth_user(user: &User) -> AuthUser {
    AuthUser {
        id: user.id,
        email: user.email.clone().unwrap_or_default(),
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        first_name: user.first_name.clone(),
        last_name: user.last_name.clone(),
        user_timezone: user.user_timezone.clone(),
        is_active: user.is_active,
        is_superuser: user.is_superuser,
        is_staff: user.is_staff,
        is_bot: user.is_bot,
        is_email_verified: user.is_email_verified,
        password: user.password.clone(),
    }
}

// ---------------------------------------------------------------------------
// Redirects
// ---------------------------------------------------------------------------

/// `get_safe_redirect_url(base_host(request, …), next_path, params)` with
/// `get_allowed_hosts()` (`path_validator.py:157-201`).
fn safe_redirect(
    ctx: &ViewCtx<'_>,
    base: &str,
    next_path: Option<&str>,
    params: &[(String, String)],
) -> Result<String, ViewError> {
    let hosts = settings_allowed_hosts(ctx.settings).map_err(|error| py_value_error(&error))?;
    get_safe_redirect_url(base, next_path, params, &hosts).map_err(|error| py_value_error(&error))
}

/// The `except AuthenticationException` branch of the four views: a 302 to the
/// base with the error dict in the query string.
fn error_redirect(
    ctx: &ViewCtx<'_>,
    flow: Flow,
    next_path: Option<&str>,
    error: &AuthFlowError,
) -> Result<ViewResponse, ViewError> {
    let base = flow_base_host(ctx, flow)?;
    let url = safe_redirect(ctx, &base, next_path, &error.redirect_params())?;
    Ok(redirect_response(&url))
}

/// `base_host(request=request, is_app=…/is_space=…)` from
/// `plane/authentication/utils/host.py:16-63`.
///
/// The app branch can answer `None` (no `WEB_URL`/`APP_BASE_URL`), which Python
/// then hands to `get_safe_redirect_url` or `rstrip("/")` — an
/// `AttributeError`, i.e. the 500 page.
fn flow_base_host(ctx: &ViewCtx<'_>, flow: Flow) -> Result<String, ViewError> {
    match view_base_host(ctx.settings, flow.login_flags()) {
        Ok(Some(base)) => Ok(base),
        Ok(None) => Err(ViewError::Unknown(
            "AttributeError: 'NoneType' object has no attribute 'rstrip'".to_string(),
        )),
        Err(error) => Err(ViewError::Unknown(error.to_string())),
    }
}

/// `HttpResponseRedirect(url)` (`django/http/response.py:190-205`): Django runs
/// `iri_to_uri` over the whole URL before writing `Location`.
fn redirect_response(url: &str) -> ViewResponse {
    ViewResponse::raw(REDIRECT_STATUS, HTML_UTF8, Vec::new())
        .with_header("Location", py::iri_to_uri(url))
}

/// `@throttle_auth_redirect(is_app=…, is_space=…)`
/// (`plane/authentication/rate_limit.py:77-93`): `AuthenticationThrottle`
/// checked by hand, then a redirect carrying `RATE_LIMIT_EXCEEDED`.
///
/// The throttle is keyed on the client IP for every caller
/// (`rate_limit.py:32-34`), so the anonymous user is the right identity: this
/// handler has not read the session.
async fn decorator_throttle(
    ctx: &ViewCtx<'_>,
    flow: Flow,
    next_path: Option<&str>,
) -> Result<Option<ViewResponse>, ViewError> {
    let redis = redis(ctx)?;
    let user = StaticUser::anonymous();
    let remote = ctx.ctx.remote_addr();
    let request = ThrottleRequest {
        settings: &ctx.settings.throttling,
        user: &user,
        headers: ctx.headers(),
        remote_addr: remote.as_deref(),
        asset_id: None,
        scope_override: None,
    };
    match plane_throttle::check_throttles(
        redis,
        &[ThrottleClass::Authentication],
        &request,
        plane_throttle::now(),
    )
    .await?
    {
        ThrottleOutcome::Allowed { .. } => Ok(None),
        ThrottleOutcome::Throttled(_) => Ok(Some(error_redirect(
            ctx,
            flow,
            next_path,
            &AuthFlowError::new(AuthErrorCode::RateLimitExceeded),
        )?)),
    }
}

// ---------------------------------------------------------------------------
// Request parsing
// ---------------------------------------------------------------------------

/// `request.POST` for a POST whose `Content-Type` is a form media type; an
/// empty `QueryDict` otherwise (`django/http/request.py:335-369`).
fn request_post(ctx: &ViewCtx<'_>) -> Result<QueryDict, ViewError> {
    if *ctx.method() != Method::POST {
        return Ok(QueryDict::default());
    }
    let media_type = ctx
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        media_type.as_str(),
        "multipart/form-data" | "application/x-www-form-urlencoded"
    ) {
        return Ok(QueryDict::default());
    }
    let classes: Vec<String> = FORM_PARSERS
        .iter()
        .map(|class| (*class).to_string())
        .collect();
    match parsers::parse(ctx.ctx, &classes)? {
        Data::Form(query) => Ok(query),
        // Unreachable: a form media type always selects one of the two form
        // parsers. An empty dict keeps the signature total.
        Data::Json(_) => Ok(QueryDict::default()),
    }
}

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

/// An `AdapterError` where only the 500 outcome is reachable.
fn internal_error(error: AdapterError) -> ViewError {
    match error {
        AdapterError::Internal(message) => ViewError::Unknown(message),
        AdapterError::Auth(error) => {
            ViewError::Unknown(format!("AuthenticationException: {}", error.message()))
        }
    }
}

/// A `plane-net`/`urlparse` `ValueError`, which Python does not catch here.
fn py_value_error(error: &plane_auth::auth_flow::PyValueError) -> ViewError {
    ViewError::Unknown(format!("ValueError: {}", error.message()))
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
