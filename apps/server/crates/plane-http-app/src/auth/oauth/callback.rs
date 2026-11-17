// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The eight OAuth *callback* views (routes 366, 368, 370, 372, 374, 376,
//! 386, 388).
//!
//! ```python
//! code = request.GET.get("code")
//! state = request.GET.get("state")
//! next_path = request.session.get("next_path")
//! if not validate_oauth_state(request=request, state=state): …error…
//! if not code: …error…
//! try:
//!     provider = XOAuthProvider(request=request, code=code, callback=post_user_auth_workflow)
//!     user = provider.authenticate()
//!     user_login(request=request, user=user, is_app=True)
//!     path = next_path if next_path else get_redirection_path(user=user)
//!     return HttpResponseRedirect(get_safe_redirect_url(base_host(…), next_path=path, params={}))
//! except AuthenticationException as e: …error…
//! ```
//!
//! The two views that do not fit that shape are the space variants (no
//! `callback=`, no `get_redirection_path`, a different success builder) and
//! the app Gitea view (a session `host` as the base and `urljoin` throughout).

use http::Method;
use plane_auth::Json;
use plane_auth::auth_flow::adapter::{
    AdapterContext, AdapterError, AuthCallback, PostUserAuthWorkflow,
};
use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::instance;
use plane_auth::auth_flow::oauth::{OAuthProvider, OauthAdapter, validate_oauth_state};
use plane_auth::auth_flow::providers::{
    GiteaProvider, GithubProvider, GitlabProvider, GoogleProvider,
};
use plane_auth::auth_flow::redirect::urlencode;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::support::{
    self, ProviderKind, Variant, error_params, json_truthy, query_get, validated_next_path,
    with_next_path,
};
use crate::auth::support as shared;

/// Register the eight callback views, in `routes.json`'s order.
pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .django_view("plane.authentication.views.app.google.GoogleCallbackEndpoint")?
        .method(Method::GET, google_app)?;
    registry
        .django_view("plane.authentication.views.app.github.GitHubCallbackEndpoint")?
        .method(Method::GET, github_app)?;
    registry
        .django_view("plane.authentication.views.app.gitlab.GitLabCallbackEndpoint")?
        .method(Method::GET, gitlab_app)?;
    registry
        .django_view("plane.authentication.views.app.gitea.GiteaCallbackEndpoint")?
        .method(Method::GET, gitea_app)?;

    registry
        .django_view("plane.authentication.views.space.google.GoogleCallbackSpaceEndpoint")?
        .method(Method::GET, google_space)?;
    registry
        .django_view("plane.authentication.views.space.github.GitHubCallbackSpaceEndpoint")?
        .method(Method::GET, github_space)?;
    registry
        .django_view("plane.authentication.views.space.gitlab.GitLabCallbackSpaceEndpoint")?
        .method(Method::GET, gitlab_space)?;
    registry
        .django_view("plane.authentication.views.space.gitea.GiteaCallbackSpaceEndpoint")?
        .method(Method::GET, gitea_space)?;
    Ok(())
}

macro_rules! callback_handler {
    ($name:ident, $provider:ident, $variant:ident) => {
        fn $name<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
            Box::pin(async move { callback(ctx, ProviderKind::$provider, Variant::$variant).await })
        }
    };
}

callback_handler!(google_app, Google, App);
callback_handler!(github_app, Github, App);
callback_handler!(gitlab_app, Gitlab, App);
callback_handler!(gitea_app, Gitea, App);
callback_handler!(google_space, Google, Space);
callback_handler!(github_space, Github, Space);
callback_handler!(gitlab_space, Gitlab, Space);
callback_handler!(gitea_space, Gitea, Space);

/// Which error branch the view is in. The Gitea views differ only in whether
/// they validate `next_path` before appending it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Branch {
    /// `not validate_oauth_state(...)` — the app Gitea view appends the **raw**
    /// `next_path` here (`views/app/gitea.py:76`).
    StateMismatch,
    /// `not code`.
    NoCode,
    /// `except AuthenticationException`.
    Exception,
}

/// One callback view.
async fn callback(
    ctx: &ViewCtx<'_>,
    provider: ProviderKind,
    variant: Variant,
) -> Result<ViewResponse, ViewError> {
    let client = ctx.database().await?;
    let settings = ctx.settings;
    let secret_key = settings.secret_key.value();
    let code = query_get(ctx, "code");
    let state = query_get(ctx, "state");
    let base = shared::base_host(settings, variant.login_flags())?;

    // `next_path = request.session.get("next_path")`, and — for the app Gitea
    // view — `host = request.session.get("host") or base_host(..., is_app=True)`
    // (`views/app/gitea.py:63-65`).
    let (next_path, session_host) = {
        let mut store = ctx.session.lock().await;
        let next_path = store
            .get(client, secret_key, "next_path")
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let host = if provider == ProviderKind::Gitea && variant.is_app() {
            store
                .get(client, secret_key, "host")
                .await
                .map_err(|error| ViewError::Unknown(error.to_string()))?
        } else {
            None
        };
        (next_path, host)
    };
    let next_path_truthy = next_path.as_ref().is_some_and(json_truthy);
    let next_path_str = next_path
        .as_ref()
        .and_then(|value| value.as_str().map(str::to_string));
    // `host = request.session.get("host") or base_host(..., is_app=True)`.
    let gitea_host = match session_host.as_ref().and_then(|value| value.as_str()) {
        Some(host) if !host.is_empty() => host.to_string(),
        _ => base.clone(),
    };

    // `validate_oauth_state(request, state)`: constant-time, deletes the stored
    // state on success and keeps it on a mismatch.
    let state_ok = {
        let mut store = ctx.session.lock().await;
        validate_oauth_state(&mut store, client, secret_key, state)
            .await
            .map_err(|error| match error {
                AdapterError::Internal(message) => ViewError::Unknown(message),
                AdapterError::Auth(error) => ViewError::Unknown(format!("{error:?}")),
            })?
    };
    if !state_ok {
        return callback_error(
            settings,
            provider,
            variant,
            &base,
            &gitea_host,
            &next_path,
            Branch::StateMismatch,
            &AuthFlowError::new(provider_error_code(provider)),
        );
    }
    let Some(code) = code else {
        return callback_error(
            settings,
            provider,
            variant,
            &base,
            &gitea_host,
            &next_path,
            Branch::NoCode,
            &AuthFlowError::new(provider_error_code(provider)),
        );
    };

    // `try:` — the provider, the exchange and the login are one block.
    let outcome = authenticate(
        ctx,
        client,
        provider,
        variant,
        code,
        &base,
        &gitea_host,
        next_path_truthy,
        next_path_str.as_deref(),
    )
    .await;
    match outcome {
        Ok(response) => Ok(response),
        Err(AdapterError::Auth(error)) => callback_error(
            settings,
            provider,
            variant,
            &base,
            &gitea_host,
            &next_path,
            Branch::Exception,
            &error,
        ),
        Err(AdapterError::Internal(message)) => Err(ViewError::Unknown(message)),
    }
}

/// The `try` block: build the provider, exchange and log in, then build the
/// success redirect.
#[allow(clippy::too_many_arguments)]
async fn authenticate(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    provider: ProviderKind,
    variant: Variant,
    code: &str,
    base: &str,
    gitea_host: &str,
    next_path_truthy: bool,
    next_path_str: Option<&str>,
) -> Result<ViewResponse, AdapterError> {
    let settings = ctx.settings;
    let adapter_context = AdapterContext {
        client,
        settings,
        ctx: ctx.ctx,
        publisher: ctx.publisher,
        cache: ctx.redis,
    };
    // The provider is constructed inside the Python `try`, with `state=None`
    // (a callback never uses the auth URL).
    let provider_impl = build_provider(ctx, client, provider).await?;
    let sync_enabled = instance::sync_enabled(client, settings, provider.name())
        .await
        .map_err(|error| AdapterError::Internal(error.to_string()))?;
    let enable_signup = instance::signup_enabled(client, settings)
        .await
        .map_err(|error| AdapterError::Internal(error.to_string()))?;
    let callback: Option<&dyn AuthCallback> = if variant.is_app() {
        Some(&PostUserAuthWorkflow)
    } else {
        None
    };
    let mut adapter = OauthAdapter::new(provider_impl, Some(code.to_string()), sync_enabled);
    let mut completed = adapter
        .authenticate(&adapter_context, callback, enable_signup)
        .await?;

    // `user_login(request=request, user=user, is_app=True/is_space=True)`: the
    // session and CSRF handles are the middleware's, so `login()`'s
    // `rotate_token` makes layer 5 write the one `csrftoken` cookie.
    shared::user_login(ctx, client, &mut completed.user, variant.login_flags())
        .await
        .map_err(|error| AdapterError::Internal(format!("{error:?}")))?;

    // `get_redirection_path(user=user)`: the app variants only, and only when
    // the session has no `next_path`.
    let redirection_path = if variant.is_app() && !next_path_truthy {
        Some(
            shared::redirection_path(client, &completed.user)
                .await
                .map_err(|error| AdapterError::Internal(format!("{error:?}")))?,
        )
    } else {
        None
    };
    let url = success_redirect(
        settings,
        provider,
        variant,
        base,
        gitea_host,
        next_path_truthy,
        next_path_str,
        redirection_path.as_deref(),
    )
    .map_err(|error| AdapterError::Internal(format!("{error:?}")))?;
    shared::redirect(&url).map_err(|error| AdapterError::Internal(format!("{error:?}")))
}

/// The success redirect, one branch per view family.
#[allow(clippy::too_many_arguments)]
fn success_redirect(
    settings: &plane_core::config::Settings,
    provider: ProviderKind,
    variant: Variant,
    base: &str,
    gitea_host: &str,
    next_path_truthy: bool,
    next_path_str: Option<&str>,
    redirection_path: Option<&str>,
) -> Result<String, ViewError> {
    if provider == ProviderKind::Gitea && variant.is_app() {
        // `path = str(validate_next_path(next_path)) if next_path else
        // get_redirection_path(user)` then `urljoin(host, path)`
        // (`views/app/gitea.py:96-101`).
        let path = if next_path_truthy {
            validated_next_path(next_path_str)?
        } else {
            redirection_path.unwrap_or_default().to_string()
        };
        return support::join(Some(gitea_host), &path);
    }
    if !variant.is_app() {
        if provider == ProviderKind::Gitea {
            // `f"{base_host(...)}{str(validate_next_path(next_path)) if
            // next_path else ''}"` (`views/space/gitea.py:101-103`).
            let path = if next_path_truthy {
                validated_next_path(next_path_str)?
            } else {
                String::new()
            };
            return Ok(format!("{base}{path}"));
        }
        // `validate_next_path(next_path)` then the concatenation
        // (`views/space/google.py:93-102`).
        return shared::space_success_url(
            settings,
            base,
            if next_path_truthy {
                next_path_str
            } else {
                None
            },
        );
    }
    let path = if next_path_truthy {
        next_path_str
    } else {
        redirection_path
    };
    shared::safe_redirect_url(settings, base, path, &[])
}

/// The `except AuthenticationException` half of every callback view.
#[allow(clippy::too_many_arguments)]
fn callback_error(
    settings: &plane_core::config::Settings,
    provider: ProviderKind,
    variant: Variant,
    base: &str,
    gitea_host: &str,
    next_path: &Option<Json>,
    branch: Branch,
    error: &AuthFlowError,
) -> Result<ViewResponse, ViewError> {
    let params = error_params(error);
    let has_next_path = next_path.as_ref().is_some_and(json_truthy);
    if provider == ProviderKind::Gitea {
        let next_path_value = match (branch, has_next_path) {
            // `params["next_path"] = str(next_path)` — raw, `views/app/gitea.py:76`.
            (Branch::StateMismatch, true) => next_path
                .as_ref()
                .map(python_str_json)
                .or(Some(String::new())),
            (_, true) => Some(validated_next_path(
                next_path.as_ref().and_then(|value| value.as_str()),
            )?),
            (_, false) => None,
        };
        let params = with_next_path(params, next_path_value.as_deref());
        let query = urlencode(&params);
        return shared::redirect(&if variant.is_app() {
            support::join(Some(gitea_host), &format!("?{query}"))?
        } else {
            format!("{base}?{query}")
        });
    }
    let next_path_str = next_path.as_ref().and_then(|value| value.as_str());
    let url = shared::safe_redirect_url(settings, base, next_path_str, &params)?;
    shared::redirect(&url)
}

/// `AuthenticationException.authentication_error_code()` for the state and
/// code checks; those two branches always raise the provider error.
fn provider_error_code(provider: ProviderKind) -> AuthErrorCode {
    match provider {
        ProviderKind::Google => AuthErrorCode::GoogleOauthProviderError,
        ProviderKind::Github => AuthErrorCode::GithubOauthProviderError,
        ProviderKind::Gitlab => AuthErrorCode::GitlabOauthProviderError,
        ProviderKind::Gitea => AuthErrorCode::GiteaOauthProviderError,
    }
}

/// `str(value)` for the raw Gitea branch.
fn python_str_json(value: &Json) -> String {
    match value {
        Json::Null => "None".to_string(),
        Json::Bool(true) => "True".to_string(),
        Json::Bool(false) => "False".to_string(),
        Json::Str(text) => text.clone(),
        Json::Int(number) => number.to_string(),
        Json::Float(number) => number.to_string(),
        // `str([...])`/`str({...})` is Python's repr; unreachable here because
        // every writer of `next_path` stores a string.
        Json::Array(_) | Json::Object(_) => format!("{value:?}"),
    }
}

/// Build the provider for a callback, with `state=None` as Python passes it.
async fn build_provider(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    provider: ProviderKind,
) -> Result<Box<dyn OAuthProvider>, AdapterError> {
    let settings = ctx.settings;
    let internal = |error: plane_auth::AuthError| AdapterError::Internal(error.to_string());
    let provider: Box<dyn OAuthProvider> = match provider {
        ProviderKind::Google => Box::new(GoogleProvider::new(
            ctx.ctx,
            &instance::google(client, settings).await.map_err(internal)?,
            None,
        )?),
        ProviderKind::Github => Box::new(GithubProvider::new(
            ctx.ctx,
            &instance::github(client, settings).await.map_err(internal)?,
            None,
        )?),
        ProviderKind::Gitlab => Box::new(GitlabProvider::new(
            ctx.ctx,
            &instance::gitlab(client, settings).await.map_err(internal)?,
            None,
        )?),
        ProviderKind::Gitea => Box::new(GiteaProvider::new(
            ctx.ctx,
            &instance::gitea(client, settings).await.map_err(internal)?,
            None,
        )?),
    };
    Ok(provider)
}
