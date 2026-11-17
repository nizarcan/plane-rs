// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/provider/oauth/google.py`.
//!
//! URLs are class constants (`google.py:22-23`), the scope is fixed (`:24`)
//! and the address is the one claim that must be the boolean `true`
//! (`:105-112`).

use plane_core::context::RequestContext;
use serde_json::{Map, Value, json};

use crate::auth_flow::adapter::{AdapterError, ProviderFuture, truthy};
use crate::auth_flow::error::{AuthErrorCode, AuthFlowError};
use crate::auth_flow::instance::GoogleConfiguration;
use crate::auth_flow::oauth::{
    OAuthHttp, OAuthProvider, TokenData, TokenRequest, oauth_redirect_uri,
};
use crate::auth_flow::redirect::urlencode;

use super::{from_timestamp, require_object};

pub const PROVIDER: &str = "google";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
pub const SCOPE: &str = "https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const LOGIN_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// `GoogleOAuthProvider` (`google.py:19-133`).
#[derive(Debug, Clone)]
pub struct GoogleProvider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    auth_url: String,
}

impl GoogleProvider {
    /// `GoogleOAuthProvider.__init__` (`google.py:26-72`), including the
    /// `GOOGLE_NOT_CONFIGURED` gate (`:41-46`).
    ///
    /// # Errors
    ///
    /// [`AuthFlowError`] holding `GOOGLE_NOT_CONFIGURED` when either half of
    /// the credential is missing.
    pub fn new(
        ctx: &RequestContext,
        config: &GoogleConfiguration,
        state: Option<&str>,
    ) -> Result<Self, AuthFlowError> {
        let (Some(client_id), Some(client_secret)) = (
            config
                .client_id
                .as_deref()
                .filter(|value| !value.is_empty()),
            config
                .client_secret
                .as_deref()
                .filter(|value| !value.is_empty()),
        ) else {
            return Err(AuthFlowError::new(AuthErrorCode::GoogleNotConfigured));
        };
        let redirect_uri = oauth_redirect_uri(ctx, PROVIDER);
        // `google.py:53-62`: client_id, scope, redirect_uri, response_type,
        // access_type, prompt, state.
        let params = vec![
            ("client_id".to_string(), client_id.to_string()),
            ("scope".to_string(), SCOPE.to_string()),
            ("redirect_uri".to_string(), redirect_uri.clone()),
            ("response_type".to_string(), "code".to_string()),
            ("access_type".to_string(), "offline".to_string()),
            ("prompt".to_string(), "consent".to_string()),
            (
                "state".to_string(),
                state.map_or_else(|| "None".to_string(), str::to_string),
            ),
        ];
        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri,
            auth_url: format!("{LOGIN_URL}?{}", urlencode(&params)),
        })
    }
}

impl OAuthProvider for GoogleProvider {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn auth_url(&self) -> &str {
        &self.auth_url
    }

    fn token_url(&self) -> &str {
        TOKEN_URL
    }

    fn userinfo_url(&self) -> &str {
        USERINFO_URL
    }

    fn authentication_error_code(&self) -> AuthErrorCode {
        AuthErrorCode::GoogleOauthProviderError
    }

    fn token_request(&self, code: Option<&str>) -> TokenRequest {
        // `google.py:75-81`: code, client_id, client_secret, redirect_uri,
        // grant_type — the dict's insertion order is the encoded body's.
        TokenRequest::new()
            .field("code", code)
            .field("client_id", Some(&self.client_id))
            .field("client_secret", Some(&self.client_secret))
            .field("redirect_uri", Some(&self.redirect_uri))
            .constant("grant_type", "authorization_code")
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
        if let Some(expires_in) = map.get("expires_in").filter(|value| truthy(Some(value))) {
            data.access_token_expired_at = Some(from_timestamp(expires_in)?);
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

/// `set_user_data` (`google.py:105-132`) without the HTTP call, so the golden
/// test can replay a recorded userinfo body.
pub(crate) fn claims(response: &Value) -> Result<Value, AdapterError> {
    let map = require_object(response, "user_info_response.get")?;
    // `if user_info_response.get("verified_email") is not True`
    // (`google.py:108-112`): an absent claim, `1` and `"true"` all fail.
    if !matches!(map.get("verified_email"), Some(Value::Bool(true))) {
        return Err(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::OauthProviderUnverifiedEmail,
        )));
    }
    let mut user = Map::new();
    user.insert("avatar".to_string(), get(map, "picture"));
    user.insert("first_name".to_string(), get(map, "given_name"));
    user.insert("last_name".to_string(), get(map, "family_name"));
    user.insert("provider_id".to_string(), get(map, "id"));
    user.insert("is_password_autoset".to_string(), Value::Bool(true));
    Ok(json!({
        "email": get(map, "email"),
        "user": Value::Object(user),
    }))
}

/// `response.get(key)` with Python's `None` for an absent key.
fn get(map: &Map<String, Value>, key: &str) -> Value {
    map.get(key).cloned().unwrap_or(Value::Null)
}
