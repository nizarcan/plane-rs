// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/provider/oauth/github.py`.
//!
//! GitHub does not return an address from `/user`, so the provider makes two
//! extra calls: the optional organization membership check (`github.py:139-152`)
//! and `/user/emails` (`:104-137`). The address rule is primary **and**
//! verified; a non-list `/user/emails` body is the provider error, not a
//! miss (`:118-125`).

use plane_core::context::RequestContext;
use serde_json::{Map, Value, json};

use crate::auth_flow::adapter::{AdapterError, ProviderFuture, python_str, truthy};
use crate::auth_flow::error::{AuthErrorCode, AuthFlowError};
use crate::auth_flow::instance::GithubConfiguration;
use crate::auth_flow::oauth::{
    OAuthHttp, OAuthProvider, TokenData, TokenRequest, oauth_redirect_uri,
};
use crate::auth_flow::redirect::urlencode;
use plane_net::preset;

use super::{from_timestamp, require_object};

pub const PROVIDER: &str = "github";
pub const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
pub const USERINFO_URL: &str = "https://api.github.com/user";
pub const ORG_MEMBERSHIP_URL: &str = "https://api.github.com/orgs";
pub const EMAILS_URL: &str = "https://api.github.com/user/emails";
pub const SCOPE: &str = "read:user user:email";
pub const ORGANIZATION_SCOPE: &str = "read:org";
const LOGIN_URL: &str = "https://github.com/login/oauth/authorize";

/// `GitHubOAuthProvider` (`github.py:26-171`).
#[derive(Debug, Clone)]
pub struct GithubProvider {
    client_id: String,
    client_secret: String,
    organization_id: Option<String>,
    redirect_uri: String,
    auth_url: String,
}

impl GithubProvider {
    /// `GitHubOAuthProvider.__init__` (`github.py:34-80`).
    ///
    /// # Errors
    ///
    /// [`AuthFlowError`] holding `GITHUB_NOT_CONFIGURED` when either half of
    /// the credential is missing (`:49-54`).
    pub fn new(
        ctx: &RequestContext,
        config: &GithubConfiguration,
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
            return Err(AuthFlowError::new(AuthErrorCode::GithubNotConfigured));
        };
        let organization_id = config
            .organization_id
            .clone()
            .filter(|value| !value.is_empty());
        let mut scope = SCOPE.to_string();
        if organization_id.is_some() {
            // `self.scope += f" {self.organization_scope}"` (`github.py:58-59`).
            scope = format!("{scope} {ORGANIZATION_SCOPE}");
        }
        let redirect_uri = oauth_redirect_uri(ctx, PROVIDER);
        // `github.py:62-69`: client_id, redirect_uri, scope, state.
        let params = vec![
            ("client_id".to_string(), client_id.to_string()),
            ("redirect_uri".to_string(), redirect_uri.clone()),
            ("scope".to_string(), scope.clone()),
            (
                "state".to_string(),
                state.map_or_else(|| "None".to_string(), str::to_string),
            ),
        ];
        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            organization_id,
            redirect_uri,
            auth_url: format!("{LOGIN_URL}?{}", urlencode(&params)),
        })
    }

    /// `is_user_in_organization` (`github.py:139-152`): a bare `GET` with no
    /// `raise_for_status`, so only the status decides.
    async fn is_user_in_organization(
        &self,
        http: &OAuthHttp<'_>,
        github_username: Option<&Value>,
    ) -> Result<bool, AdapterError> {
        let organization_id = self.organization_id.as_deref().unwrap_or_default();
        let url = membership_url(organization_id, github_username);
        let headers = [("Authorization".to_string(), http.bearer())];
        let status = http
            .get_status(&preset::GITHUB_ORG_MEMBERSHIP, &url, &headers)
            .await?;
        Ok(is_member(status))
    }

    /// `__get_email` (`github.py:104-137`).
    async fn email(&self, http: &OAuthHttp<'_>) -> Result<Value, AdapterError> {
        let headers = [
            ("Authorization".to_string(), http.bearer()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        let response = http
            .get_unchecked(&preset::GITHUB_EMAILS, EMAILS_URL, &headers)
            .await
            .map_err(|error| {
                tracing::warn!(target: "plane.auth", %error, "Error getting email from GitHub");
                AdapterError::Auth(AuthFlowError::new(AuthErrorCode::GithubOauthProviderError))
            })?;
        // `emails_response = … .json()` is inside the Python `try`, and
        // requests 2.33's `JSONDecodeError` is a `RequestException`, so a
        // non-JSON body is the provider error before the list check.
        let emails = response.json().map_err(|_| {
            AdapterError::Auth(AuthFlowError::new(AuthErrorCode::GithubOauthProviderError))
        })?;
        select_email(&emails)
    }
}

/// `__get_email`'s value half: the list check and the
/// `next((e["email"] for e in … if e.get("primary") and e.get("verified")),
/// None)` generator (`github.py:114-132`).
pub(crate) fn select_email(emails: &Value) -> Result<Value, AdapterError> {
    let Value::Array(emails) = emails else {
        tracing::error!(
            target: "plane.auth",
            "Unexpected response format from GitHub emails API"
        );
        return Err(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::GithubOauthProviderError,
        )));
    };
    for entry in emails {
        let Value::Object(entry) = entry else {
            return Err(AdapterError::Internal(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                value_type_name(entry)
            )));
        };
        if truthy(entry.get("primary")) && truthy(entry.get("verified")) {
            // `e["email"]` — a matching entry without the key is a KeyError.
            return entry
                .get("email")
                .cloned()
                .ok_or_else(|| AdapterError::Internal("KeyError: 'email'".to_string()));
        }
    }
    tracing::error!(
        target: "plane.auth",
        "No primary verified email found for GitHub user"
    );
    Err(AdapterError::Auth(AuthFlowError::new(
        AuthErrorCode::OauthProviderUnverifiedEmail,
    )))
}

impl OAuthProvider for GithubProvider {
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
        AuthErrorCode::GithubOauthProviderError
    }

    fn token_request(&self, code: Option<&str>) -> TokenRequest {
        // `github.py:84-89`: client_id, client_secret, code, redirect_uri, and
        // `headers={"Accept": "application/json"}` (`:90`).
        TokenRequest::new()
            .field("client_id", Some(&self.client_id))
            .field("client_secret", Some(&self.client_secret))
            .field("code", code)
            .field("redirect_uri", Some(&self.redirect_uri))
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
        http: &'a OAuthHttp<'a>,
        response: &'a Value,
    ) -> ProviderFuture<'a, Value> {
        Box::pin(async move {
            let map = require_object(response, "user_info_response.get")?;
            if self.organization_id.is_some()
                && !self.is_user_in_organization(http, map.get("login")).await?
            {
                // Do not log the organization id or the login.
                tracing::warn!(target: "plane.auth", "User is not in organization");
                return Err(AdapterError::Auth(AuthFlowError::new(
                    AuthErrorCode::GithubUserNotInOrg,
                )));
            }
            let email = self.email(http).await?;
            tracing::debug!(target: "plane.auth", "Email found");
            claims(response, email)
        })
    }
}

/// `set_user_data`'s dict (`github.py:158-171`) with the address already
/// fetched.
pub(crate) fn claims(response: &Value, email: Value) -> Result<Value, AdapterError> {
    let map = require_object(response, "user_info_response.get")?;
    let mut user = Map::new();
    user.insert("provider_id".to_string(), get(map, "id"));
    user.insert("email".to_string(), email.clone());
    user.insert("avatar".to_string(), get(map, "avatar_url"));
    user.insert("first_name".to_string(), get(map, "name"));
    // GitHub has no `family_name`; the Python reads it anyway, so it is
    // always `None` → `""` in `base.py:371-373`.
    user.insert("last_name".to_string(), get(map, "family_name"));
    user.insert("is_password_autoset".to_string(), Value::Bool(true));
    Ok(json!({
        "email": email,
        "user": Value::Object(user),
    }))
}

/// `f"{self.org_membership_url}/{self.organization_id}/memberships/{login}"`
/// (`github.py:141-144`). A non-string login is Python's `str()`.
pub(crate) fn membership_url(organization_id: &str, login: Option<&Value>) -> String {
    let username = login.map_or_else(|| "None".to_string(), python_str);
    format!("{ORG_MEMBERSHIP_URL}/{organization_id}/memberships/{username}")
}

/// `response.status_code == 200` (`github.py:151`). No `raise_for_status`.
pub(crate) fn is_member(status: u16) -> bool {
    status == 200
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
