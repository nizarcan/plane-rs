// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/provider/oauth/gitea.py`.
//!
//! Three things are unique to Gitea:
//!
//! * `GITEA_HOST` must parse with an `http`/`https` scheme and loses its
//!   trailing slashes (`gitea.py:42-51`); a bad host is `GITEA_NOT_CONFIGURED`
//!   so the reason does not leak into the query string.
//! * the address always comes from `/user/emails` (`:117-155`), because the
//!   user object carries no verification flag: primary+verified, then any
//!   verified, never an unverified one. The three failure branches carry
//!   messages of their own ("Failed to fetch emails", "No emails found",
//!   "Exception occurred while fetching emails").
//! * `provider_id` is `str(user_info_response.get("id"))` (`:166`) and the
//!   first name falls back from `full_name` to `login` (`:167`).

use plane_core::context::RequestContext;
use plane_net::preset;
use serde_json::{Map, Value, json};

use crate::auth_flow::adapter::{AdapterError, ProviderFuture, python_str, truthy};
use crate::auth_flow::error::{AuthErrorCode, AuthFlowError};
use crate::auth_flow::instance::GiteaConfiguration;
use crate::auth_flow::oauth::{
    OAuthHttp, OAuthProvider, TokenData, TokenRequest, oauth_redirect_uri,
};
use crate::auth_flow::redirect::urlencode;

use super::{from_timestamp, now_plus_seconds, require_object};

pub const PROVIDER: &str = "gitea";
pub const SCOPE: &str = "openid email profile read:user";

/// `GiteaOAuthProvider` (`gitea.py:18-175`).
#[derive(Debug, Clone)]
pub struct GiteaProvider {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    token_url: String,
    userinfo_url: String,
    auth_url: String,
}

impl GiteaProvider {
    /// `GiteaOAuthProvider.__init__` (`gitea.py:24-86`).
    ///
    /// # Errors
    ///
    /// [`AuthFlowError`] holding `GITEA_NOT_CONFIGURED` for a missing id,
    /// secret or host (`:42-46`) and for a host that does not parse to an
    /// `http`/`https` URL (`:51-56`).
    pub fn new(
        ctx: &RequestContext,
        config: &GiteaConfiguration,
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
            return Err(AuthFlowError::new(AuthErrorCode::GiteaNotConfigured));
        };
        // `urlparse(GITEA_HOST)`: a missing or unsupported scheme is
        // `GITEA_NOT_CONFIGURED` with the plain message (`:48-56`).
        let Ok(parsed) = plane_net::pyurl::urlsplit(host) else {
            return Err(AuthFlowError::new(AuthErrorCode::GiteaNotConfigured));
        };
        if parsed.scheme != "http" && parsed.scheme != "https" {
            return Err(AuthFlowError::new(AuthErrorCode::GiteaNotConfigured));
        }
        let host = host.trim_end_matches('/');
        let token_url = format!("{host}/login/oauth/access_token");
        let userinfo_url = format!("{host}/api/v1/user");
        let redirect_uri = oauth_redirect_uri(ctx, PROVIDER);
        // `gitea.py:65-71`: client_id, scope, redirect_uri, response_type,
        // state.
        let params = vec![
            ("client_id".to_string(), client_id.to_string()),
            ("scope".to_string(), SCOPE.to_string()),
            ("redirect_uri".to_string(), redirect_uri.clone()),
            ("response_type".to_string(), "code".to_string()),
            (
                "state".to_string(),
                state.map_or_else(|| "None".to_string(), str::to_string),
            ),
        ];
        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri,
            auth_url: format!("{host}/login/oauth/authorize?{}", urlencode(&params)),
            token_url,
            userinfo_url,
        })
    }

    /// `__get_email` (`gitea.py:117-155`).
    async fn email(&self, http: &OAuthHttp<'_>) -> Result<Value, AdapterError> {
        let headers = [
            ("Authorization".to_string(), http.bearer()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        let url = format!("{}/emails", self.userinfo_url);
        let response = http
            .get_unchecked(&preset::GITEA_EMAILS, &url, &headers)
            .await
            .map_err(|error| {
                tracing::warn!(target: "plane.auth", %error, "Error getting email from Gitea");
                gitea_error("GITEA_OAUTH_PROVIDER_ERROR: Exception occurred while fetching emails")
            })?;
        if !response.ok() {
            return Err(gitea_error(
                "GITEA_OAUTH_PROVIDER_ERROR: Failed to fetch emails",
            ));
        }
        // `response.json()` is inside the Python `try`; requests 2.33's
        // `JSONDecodeError` is a `RequestException`, so a non-JSON body takes
        // the transport branch (`gitea.py:146-155`).
        let emails = response.json().map_err(|_| {
            gitea_error("GITEA_OAUTH_PROVIDER_ERROR: Exception occurred while fetching emails")
        })?;
        select_email(&emails)
    }
}

/// `__get_email`'s value half (`gitea.py:126-154`): the empty check, the
/// primary+verified pass and the any-verified fallback.
pub(crate) fn select_email(emails: &Value) -> Result<Value, AdapterError> {
    if !truthy(Some(emails)) {
        return Err(gitea_error("GITEA_OAUTH_PROVIDER_ERROR: No emails found"));
    }
    // `for e in emails_response`: a dict iterates its keys and a string its
    // characters, so `e.get` is an AttributeError — a 500. Only a list works.
    let Value::Array(emails) = emails else {
        return Err(AdapterError::Internal(format!(
            "AttributeError: '{}' object has no attribute 'get'",
            value_type_name(emails)
        )));
    };
    let entries = emails
        .iter()
        .map(|entry| match entry {
            Value::Object(map) => Ok(map),
            other => Err(AdapterError::Internal(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                value_type_name(other)
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let email = entries
        .iter()
        .find(|entry| truthy(entry.get("primary")) && truthy(entry.get("verified")))
        .and_then(|entry| entry.get("email").cloned());
    let email = match email {
        Some(email) if truthy(Some(&email)) => email,
        _ => match entries
            .iter()
            .find(|entry| truthy(entry.get("verified")))
            .and_then(|entry| entry.get("email").cloned())
        {
            Some(email) if truthy(Some(&email)) => email,
            _ => {
                return Err(AdapterError::Auth(AuthFlowError::new(
                    AuthErrorCode::OauthProviderUnverifiedEmail,
                )));
            }
        },
    };
    Ok(email)
}

impl OAuthProvider for GiteaProvider {
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
        AuthErrorCode::GiteaOauthProviderError
    }

    fn token_request(&self, code: Option<&str>) -> TokenRequest {
        // `gitea.py:88-95`: code, client_id, client_secret, redirect_uri,
        // grant_type, with `Accept: application/json` (`:96-97`).
        TokenRequest::new()
            .field("code", code)
            .field("client_id", Some(&self.client_id))
            .field("client_secret", Some(&self.client_secret))
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
        // `datetime.now(tz=utc) + timedelta(seconds=expires_in)`
        // (`gitea.py:97-101`): the clock, not the provider's epoch.
        if let Some(expires_in) = map.get("expires_in").filter(|value| truthy(Some(value))) {
            data.access_token_expired_at = Some(now_plus_seconds(expires_in)?);
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
        http: &'a OAuthHttp<'a>,
        response: &'a Value,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let _ = require_object(response, "user_info_response.get")?;
            // Always `/user/emails`, which enforces the verified rule
            // (`gitea.py:157-164`).
            let email = self.email(http).await?;
            claims(response, email)
        })
    }
}

/// `set_user_data`'s dict (`gitea.py:166-175`) with the address already
/// fetched.
pub(crate) fn claims(response: &Value, email: Value) -> Result<Value, AdapterError> {
    let map = require_object(response, "user_info_response.get")?;
    let first_name = match map.get("full_name") {
        Some(value) if truthy(Some(value)) => value.clone(),
        _ => map.get("login").cloned().unwrap_or(Value::Null),
    };
    let mut user = Map::new();
    // `str(user_info_response.get("id"))` (`:166`): `None` is the string
    // `"None"`.
    user.insert(
        "provider_id".to_string(),
        Value::String(map.get("id").map_or_else(|| "None".to_string(), python_str)),
    );
    user.insert("email".to_string(), email.clone());
    user.insert("avatar".to_string(), get(map, "avatar_url"));
    user.insert("first_name".to_string(), first_name);
    // "Gitea doesn't provide separate first/last name" (`:168`).
    user.insert("last_name".to_string(), Value::String(String::new()));
    user.insert("is_password_autoset".to_string(), Value::Bool(true));
    Ok(json!({
        "email": email,
        "user": Value::Object(user),
    }))
}

fn gitea_error(message: &str) -> AdapterError {
    AdapterError::Auth(
        AuthFlowError::new(AuthErrorCode::GiteaOauthProviderError).with_message(message),
    )
}

/// `response.get(key)` with Python's `None` for an absent key.
fn get(map: &Map<String, Value>, key: &str) -> Value {
    map.get(key).cloned().unwrap_or(Value::Null)
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}
