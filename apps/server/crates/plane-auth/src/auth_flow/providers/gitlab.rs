// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/provider/oauth/gitlab.py`.
//!
//! The host is configurable (`GITLAB_HOST`, default `https://gitlab.com` at
//! the call site, `gitlab.py:26-40`), so the token and userinfo URLs are
//! built from it. The address rule is a truthy `confirmed_at`
//! (`gitlab.py:111-117`).

use plane_core::context::RequestContext;
use serde_json::{Map, Value, json};

use crate::auth_flow::adapter::{AdapterError, ProviderFuture, truthy};
use crate::auth_flow::error::{AuthErrorCode, AuthFlowError};
use crate::auth_flow::instance::GitlabConfiguration;
use crate::auth_flow::oauth::{
    OAuthHttp, OAuthProvider, TokenData, TokenRequest, oauth_redirect_uri,
};
use crate::auth_flow::redirect::urlencode;

use super::{add_timestamps, from_timestamp, require_object};

pub const PROVIDER: &str = "gitlab";
pub const SCOPE: &str = "read_user";

/// `GitLabOAuthProvider` (`gitlab.py:19-136`).
#[derive(Debug, Clone)]
pub struct GitlabProvider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    token_url: String,
    userinfo_url: String,
    auth_url: String,
}

impl GitlabProvider {
    /// `GitLabOAuthProvider.__init__` (`gitlab.py:25-75`).
    ///
    /// # Errors
    ///
    /// [`AuthFlowError`] holding `GITLAB_NOT_CONFIGURED` when the id, the
    /// secret or the host is missing (`:47-52`).
    pub fn new(
        ctx: &RequestContext,
        config: &GitlabConfiguration,
        state: Option<&str>,
    ) -> Result<Self, AuthFlowError> {
        let (Some(client_id), Some(client_secret), Some(host)) = (
            config
                .client_id
                .as_deref()
                .filter(|value| !value.is_empty()),
            config
                .client_secret
                .as_deref()
                .filter(|value| !value.is_empty()),
            config.host.as_deref().filter(|value| !value.is_empty()),
        ) else {
            return Err(AuthFlowError::new(AuthErrorCode::GitlabNotConfigured));
        };
        // The host is joined verbatim: no trailing-slash normalisation
        // (`gitlab.py:43-45`).
        let token_url = format!("{host}/oauth/token");
        let userinfo_url = format!("{host}/api/v4/user");
        let redirect_uri = oauth_redirect_uri(ctx, PROVIDER);
        // `gitlab.py:55-61`: client_id, redirect_uri, response_type, scope,
        // state.
        let params = vec![
            ("client_id".to_string(), client_id.to_string()),
            ("redirect_uri".to_string(), redirect_uri.clone()),
            ("response_type".to_string(), "code".to_string()),
            ("scope".to_string(), SCOPE.to_string()),
            (
                "state".to_string(),
                state.map_or_else(|| "None".to_string(), str::to_string),
            ),
        ];
        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri,
            auth_url: format!("{host}/oauth/authorize?{}", urlencode(&params)),
            token_url,
            userinfo_url,
        })
    }
}

impl OAuthProvider for GitlabProvider {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn auth_url(&self) -> &str {
        &self.auth_url
    }

    fn token_url(&self) -> &str {
        &self.token_url
    }

    fn userinfo_url(&self) -> &str {
        &self.userinfo_url
    }

    fn authentication_error_code(&self) -> AuthErrorCode {
        AuthErrorCode::GitlabOauthProviderError
    }

    fn token_request(&self, code: Option<&str>) -> TokenRequest {
        // `gitlab.py:77-84`: client_id, client_secret, code, redirect_uri,
        // grant_type, with `headers={"Accept": "application/json"}` (`:85`).
        TokenRequest::new()
            .field("client_id", Some(&self.client_id))
            .field("client_secret", Some(&self.client_secret))
            .field("code", code)
            .field("redirect_uri", Some(&self.redirect_uri))
            .constant("grant_type", "authorization_code")
            .header("Accept", "application/json")
    }

    fn token_data(&self, response: &Value) -> Result<TokenData, AdapterError> {
        let map = require_object(response, "token_response.get")?;
        let mut data = TokenData::new(
            map.get("access_token").cloned().unwrap_or(Value::Null),
            map.get("refresh_token").cloned().unwrap_or(Value::Null),
            map.get("id_token")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
        );
        // `created_at + expires_in` (`gitlab.py:99-104`): the sum needs both
        // keys, so an `expires_in` without a `created_at` is a TypeError.
        if let Some(expires_in) = map.get("expires_in").filter(|value| truthy(Some(value))) {
            let created_at = map.get("created_at").ok_or_else(|| {
                AdapterError::Internal(
                    "TypeError: unsupported operand type(s) for +: 'NoneType' and 'int'"
                        .to_string(),
                )
            })?;
            data.access_token_expired_at = Some(add_timestamps(created_at, expires_in)?);
        }
        if let Some(expires) = map
            .get("refresh_token_expired_at")
            .filter(|value| truthy(Some(value)))
        {
            data.refresh_token_expired_at = Some(from_timestamp(expires)?);
        }
        Ok(data)
    }

    fn user_data<'a>(
        &'a self,
        _http: &'a OAuthHttp<'a>,
        response: &'a Value,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move { claims(response) })
    }
}

/// `set_user_data` (`gitlab.py:108-134`) without the HTTP call, so the golden
/// test can replay a recorded userinfo body.
pub(crate) fn claims(response: &Value) -> Result<Value, AdapterError> {
    let map = require_object(response, "user_info_response.get")?;
    // `if not user_info_response.get("confirmed_at")` (`:111-117`).
    if !map
        .get("confirmed_at")
        .is_some_and(|value| truthy(Some(value)))
    {
        return Err(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::OauthProviderUnverifiedEmail,
        )));
    }
    let email = map.get("email").cloned().unwrap_or(Value::Null);
    let mut user = Map::new();
    user.insert("provider_id".to_string(), get(map, "id"));
    user.insert("email".to_string(), email.clone());
    user.insert("avatar".to_string(), get(map, "avatar_url"));
    user.insert("first_name".to_string(), get(map, "name"));
    user.insert("last_name".to_string(), get(map, "family_name"));
    user.insert("is_password_autoset".to_string(), Value::Bool(true));
    Ok(json!({
        "email": email,
        "user": Value::Object(user),
    }))
}

/// `response.get(key)` with Python's `None` for an absent key.
fn get(map: &Map<String, Value>, key: &str) -> Value {
    map.get(key).cloned().unwrap_or(Value::Null)
}
