// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The eight OAuth *initiate* views (routes 365, 367, 369, 371, 373, 375,
//! 385, 387).
//!
//! ```python
//! request.session["host"] = base_host(request=request, is_app=True)
//! next_path = request.GET.get("next_path")
//! if next_path:
//!     request.session["next_path"] = str(next_path)          # app only
//! instance = Instance.objects.first()
//! if instance is None or not instance.is_setup_done: …
//! try:
//!     state = uuid.uuid4().hex
//!     provider = XOAuthProvider(request=request, state=state)
//!     request.session["state"] = state
//!     return HttpResponseRedirect(provider.get_auth_url())
//! except AuthenticationException as e: …
//! ```
//!
//! The instance gate runs **before** the provider is constructed, so an
//! unconfigured provider on a fresh instance never reaches
//! `*_NOT_CONFIGURED`. The state is generated before the provider and stored
//! only after it succeeds; the session has `host`/`next_path` either way.

use http::Method;
use plane_auth::Json;
use plane_auth::auth_flow::adapter::AdapterError;
use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::instance;
use plane_auth::auth_flow::oauth::{OAUTH_STATE_SESSION_KEY, OAuthProvider};
use plane_auth::auth_flow::providers::{
    GiteaProvider, GithubProvider, GitlabProvider, GoogleProvider,
};
use plane_auth::auth_flow::redirect::urlencode;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::support::{
    self, ProviderKind, Variant, error_params, query_get, validated_next_path, with_next_path,
};
use crate::auth::support as shared;

/// Register the eight initiate views, in `routes.json`'s order.
pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .django_view("plane.authentication.views.app.google.GoogleOauthInitiateEndpoint")?
        .method(Method::GET, google_app)?;
    registry
        .django_view("plane.authentication.views.app.github.GitHubOauthInitiateEndpoint")?
        .method(Method::GET, github_app)?;
    registry
        .django_view("plane.authentication.views.app.gitlab.GitLabOauthInitiateEndpoint")?
        .method(Method::GET, gitlab_app)?;
    registry
        .django_view("plane.authentication.views.app.gitea.GiteaOauthInitiateEndpoint")?
        .method(Method::GET, gitea_app)?;

    registry
        .django_view("plane.authentication.views.space.google.GoogleOauthInitiateSpaceEndpoint")?
        .method(Method::GET, google_space)?;
    registry
        .django_view("plane.authentication.views.space.github.GitHubOauthInitiateSpaceEndpoint")?
        .method(Method::GET, github_space)?;
    registry
        .django_view("plane.authentication.views.space.gitlab.GitLabOauthInitiateSpaceEndpoint")?
        .method(Method::GET, gitlab_space)?;
    registry
        .django_view("plane.authentication.views.space.gitea.GiteaOauthInitiateSpaceEndpoint")?
        .method(Method::GET, gitea_space)?;
    Ok(())
}

macro_rules! initiate_handler {
    ($name:ident, $provider:ident, $variant:ident) => {
        fn $name<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
            Box::pin(async move { initiate(ctx, ProviderKind::$provider, Variant::$variant).await })
        }
    };
}

initiate_handler!(google_app, Google, App);
initiate_handler!(github_app, Github, App);
initiate_handler!(gitlab_app, Gitlab, App);
initiate_handler!(gitea_app, Gitea, App);
initiate_handler!(google_space, Google, Space);
initiate_handler!(github_space, Github, Space);
initiate_handler!(gitlab_space, Gitlab, Space);
initiate_handler!(gitea_space, Gitea, Space);

/// One initiate view (`views/app/google.py:28-60` and its siblings).
async fn initiate(
    ctx: &ViewCtx<'_>,
    provider: ProviderKind,
    variant: Variant,
) -> Result<ViewResponse, ViewError> {
    let client = ctx.database().await?;
    let settings = ctx.settings;
    let secret_key = settings.secret_key.value();
    let base = shared::base_host(settings, variant.login_flags())?;

    // `next_path = request.GET.get("next_path")`, read before the session is
    // touched and kept for the error redirects.
    let next_path = query_get(ctx, "next_path");
    let has_next_path = next_path.is_some_and(|value| !value.is_empty());

    // `request.session["host"] = base_host(...)`, and — for the app variants
    // and for Gitea — the `next_path`.
    {
        let mut store = ctx.session.lock().await;
        store
            .set(client, secret_key, "host", Json::Str(base.clone()))
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        if has_next_path && (variant.is_app() || provider == ProviderKind::Gitea) {
            // `str(next_path)` for the app variants; Gitea stores
            // `str(validate_next_path(next_path))`
            // (`views/{app,space}/gitea.py:35-38`).
            let stored = if provider == ProviderKind::Gitea {
                validated_next_path(next_path)?
            } else {
                next_path.unwrap_or_default().to_string()
            };
            store
                .set(client, secret_key, "next_path", Json::Str(stored))
                .await
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
        }
    }

    // `instance is None or not instance.is_setup_done`.
    let configured = instance::is_setup_done(client)
        .await
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    if !configured {
        let error = AuthFlowError::new(AuthErrorCode::InstanceNotConfigured);
        return error_redirect(settings, provider, variant, &base, next_path, &error);
    }

    // `state = uuid.uuid4().hex`, then the provider.
    let state = uuid::Uuid::new_v4().simple().to_string();
    match build_provider(ctx, client, provider, &state).await {
        Ok(auth_url) => {
            {
                let mut store = ctx.session.lock().await;
                store
                    .set(
                        client,
                        secret_key,
                        OAUTH_STATE_SESSION_KEY,
                        Json::Str(state.clone()),
                    )
                    .await
                    .map_err(|error| ViewError::Unknown(error.to_string()))?;
            }
            shared::redirect(&auth_url)
        }
        Err(AdapterError::Auth(error)) => {
            error_redirect(settings, provider, variant, &base, next_path, &error)
        }
        Err(AdapterError::Internal(message)) => Err(ViewError::Unknown(message)),
    }
}

/// Build the provider, which is also its `*_NOT_CONFIGURED` gate.
///
/// The configuration read itself is a database query in the view, so its
/// failure is Python's uncaught exception — [`AdapterError::Internal`], not a
/// redirect.
pub(crate) async fn build_provider(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    provider: ProviderKind,
    state: &str,
) -> Result<String, AdapterError> {
    let settings = ctx.settings;
    let internal = |error: plane_auth::AuthError| AdapterError::Internal(error.to_string());
    let auth_url = match provider {
        ProviderKind::Google => {
            let config = instance::google(client, settings).await.map_err(internal)?;
            GoogleProvider::new(ctx.ctx, &config, Some(state))?
                .auth_url()
                .to_string()
        }
        ProviderKind::Github => {
            let config = instance::github(client, settings).await.map_err(internal)?;
            GithubProvider::new(ctx.ctx, &config, Some(state))?
                .auth_url()
                .to_string()
        }
        ProviderKind::Gitlab => {
            let config = instance::gitlab(client, settings).await.map_err(internal)?;
            GitlabProvider::new(ctx.ctx, &config, Some(state))?
                .auth_url()
                .to_string()
        }
        ProviderKind::Gitea => {
            let config = instance::gitea(client, settings).await.map_err(internal)?;
            GiteaProvider::new(ctx.ctx, &config, Some(state))?
                .auth_url()
                .to_string()
        }
    };
    Ok(auth_url)
}

/// The `except AuthenticationException` half of every initiate view.
///
/// The app variants and the space Google/GitHub/GitLab views pass the **raw**
/// query value to `get_safe_redirect_url`; the two Gitea views append the
/// **validated** one and build the URL themselves
/// (`views/app/gitea.py:40-47`, `views/space/gitea.py:44-52`).
pub(crate) fn error_redirect(
    settings: &plane_core::config::Settings,
    provider: ProviderKind,
    variant: Variant,
    base: &str,
    next_path: Option<&str>,
    error: &AuthFlowError,
) -> Result<ViewResponse, ViewError> {
    let params = error_params(error);
    let has_next_path = next_path.is_some_and(|value| !value.is_empty());
    if provider == ProviderKind::Gitea {
        let params = if has_next_path {
            with_next_path(params, Some(&validated_next_path(next_path)?))
        } else {
            params
        };
        let query = format!("?{}", urlencode(&params));
        return shared::redirect(&if variant.is_app() {
            support::join(Some(base), &query)?
        } else {
            format!("{base}{query}")
        });
    }
    let url = shared::safe_redirect_url(settings, base, next_path, &params)?;
    shared::redirect(&url)
}
