// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/adapter/oauth.py` and the OAuth half of
//! `adapter/base.py`: the state check, the token/userinfo exchange, the
//! `accounts` upsert, the IDP sync and the avatar download/upload.
//!
//! The four provider modules (`provider/oauth/{google,github,gitlab,gitea}.py`)
//! are a separate set: they implement [`OAuthProvider`], which is only the
//! provider-specific half of `OauthAdapter` (the URLs, the token request, the
//! claim mapping). Everything they share — the HTTP calls, `token_data`'s
//! shape, `complete_login_or_signup`, the account row, the sync and the avatar
//! — lives here.
//!
//! # What the Python does that must be kept
//!
//! * `validate_oauth_state` is constant-time, **deletes** the stored state on
//!   success and **keeps** it on a mismatch, so a user who mistypes a callback
//!   can retry the login they already started (`utils/oauth_state.py:9-19`).
//! * The token and userinfo calls carry no SSRF guard and no timeout
//!   (`adapter/oauth.py:75-96`): `requests.post`/`get` with environment
//!   proxies and 30 redirects — `plane_net::preset::OAUTH_TOKEN` /
//!   `OAUTH_USERINFO` (`DECISIONS.md` D9).
//! * `raise_for_status()` is inside the `try`, so a 4xx/5xx is the provider's
//!   `*_OAUTH_PROVIDER_ERROR`, while a **non-JSON** body is
//!   `response.json()`'s `ValueError` and therefore an uncaught 500
//!   (`oauth.py:75-96`).
//! * `create_update_account` swallows `DatabaseError`/`IntegrityError` only
//!   (`oauth.py:131-132`) — the `NOT NULL` violation from a missing
//!   `access_token`/`provider_account_id` is a logged no-op, not a 500.
//! * The avatar path is entirely best-effort: `download_and_upload_avatar`
//!   wraps everything in `except Exception` and returns `None`, so a broken
//!   storage configuration falls back to the provider's URL
//!   (`base.py:136-229`).
//! * `sync_user_data` runs only for an existing user with
//!   `ENABLE_<PROVIDER>_SYNC == "1"` (`base.py:122-134,390-392`).

use chrono::{DateTime, SubsecRound, Utc};
use deadpool_postgres::Client;
use http::Method;
use plane_core::context::RequestContext;
use plane_db::JsonText;
use plane_db::models::user::User;
use plane_net::{FetchError, HttpEngine, OutboundRequest, Preset, SsrfPolicy, preset};
use plane_serde::Dialect;
use plane_storage::{RequestInfo, Storage};
use serde_json::Value;
use uuid::Uuid;

use crate::auth_flow::adapter::{
    AdapterContext, AdapterError, AuthCallback, AuthProvider, CompletedLogin, ProviderFuture,
    complete_login_or_signup, python_str, truthy,
};
use crate::auth_flow::error::{AuthErrorCode, AuthFlowError};
use crate::json::Json;
use crate::random::get_random_string;
use crate::session::SessionStore;

/// The session key the initiate views store the state under
/// (`views/app/google.py:42`, `.../github.py:42`, `.../gitlab.py:42`,
/// `.../gitea.py:42`).
pub const OAUTH_STATE_SESSION_KEY: &str = "state";

/// `string.ascii_letters`.
const ASCII_LETTERS: &str = concat!("abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ",);

/// `secrets.compare_digest(a, b)` for two UTF-8 byte strings.
///
/// CPython's `_tscmp` XORs over the **shorter** input and folds the length
/// mismatch into the same accumulator, so it is constant-time in the contents
/// and in the shorter length; this is that loop, not a `memcmp` that exits at
/// the first difference.
fn compare_digest(left: &[u8], right: &[u8]) -> bool {
    let mut result = u8::from(left.len() != right.len());
    for (a, b) in left.iter().zip(right.iter()) {
        result |= a ^ b;
    }
    result == 0
}

/// The decision half of `validate_oauth_state` (`utils/oauth_state.py:10-18`):
/// `state and expected and secrets.compare_digest(state.encode(),
/// expected.encode())`.
///
/// A non-string truthy `expected` is Python's `AttributeError: 'int' object
/// has no attribute 'encode'`, i.e. a 500; it is not a silent `false`.
///
/// # Errors
///
/// [`AdapterError::Internal`] for a stored state that is not a string.
pub fn oauth_state_matches(
    state: Option<&str>,
    expected: Option<&Json>,
) -> Result<bool, AdapterError> {
    let Some(state) = state.filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let Some(expected) = expected.filter(|value| json_truthy(value)) else {
        return Ok(false);
    };
    let Json::Str(expected) = expected else {
        let kind = python_type_name(expected);
        return Err(AdapterError::Internal(format!(
            "AttributeError: '{kind}' object has no attribute 'encode'"
        )));
    };
    Ok(compare_digest(state.as_bytes(), expected.as_bytes()))
}

/// Python truthiness for the session's JSON values.
fn json_truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => *value != 0,
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => !value.is_empty(),
        Json::Array(items) => !items.is_empty(),
        Json::Object(entries) => !entries.is_empty(),
    }
}

/// `type(value).__name__` for the non-string states a session can hold.
fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::Array(_) => "list",
        Json::Object(_) => "dict",
    }
}

/// `validate_oauth_state(request, state)` (`utils/oauth_state.py:9-19`).
///
/// Returns `true` only when `state` matches the non-empty state stored in the
/// session at initiation; a match removes the key so it can only be used once.
/// A missing or mismatched state leaves the stored one in place for the login
/// it belongs to.
///
/// # Errors
///
/// A session-store failure, or the `AttributeError` above.
pub async fn validate_oauth_state(
    store: &mut SessionStore,
    client: &Client,
    secret_key: &str,
    state: Option<&str>,
) -> Result<bool, AdapterError> {
    let expected = store
        .get(client, secret_key, OAUTH_STATE_SESSION_KEY)
        .await?;
    if !oauth_state_matches(state, expected.as_ref())? {
        return Ok(false);
    }
    store
        .remove(client, secret_key, OAUTH_STATE_SESSION_KEY)
        .await?;
    Ok(true)
}

/// The `redirect_uri` every provider builds (`google.py:50`, `github.py:62`,
/// `gitlab.py:56`, `gitea.py:64`):
///
/// ```python
/// f"""{"https" if request.is_secure() else "http"}://{request.get_host()}/auth/<p>/callback/"""
/// ```
///
/// `request.get_host()` was already validated by `CommonMiddleware` (a bad
/// host is a 400 before the view runs), which is why this reads the `Host`
/// header directly (`crates/plane-drf/src/auth.rs:223-226`).
#[must_use]
pub fn oauth_redirect_uri(ctx: &RequestContext, provider: &str) -> String {
    let scheme = if ctx.is_secure { "https" } else { "http" };
    let host = ctx.header("host").unwrap_or_default();
    format!("{scheme}://{host}/auth/{provider}/callback/")
}

/// `check_sync_enabled`'s provider → configuration-key map
/// (`base.py:122-134`). `None` for an unknown provider, which is `False`.
#[must_use]
pub fn sync_flag_name(provider: &str) -> Option<&'static str> {
    match provider {
        "google" => Some("ENABLE_GOOGLE_SYNC"),
        "github" => Some("ENABLE_GITHUB_SYNC"),
        "gitlab" => Some("ENABLE_GITLAB_SYNC"),
        "gitea" => Some("ENABLE_GITEA_SYNC"),
        _ => None,
    }
}

/// The form body and headers of a token request: `requests.post(token_url,
/// data=data, headers=headers)`.
///
/// Field order is the Python dict's, because the encoded body is what the
/// provider sees. A `None` value is `urlencode`'s `str(None)`, i.e. `"None"` —
/// reachable through `code=None` and a missing `client_secret`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenRequest {
    pub data: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
}

impl TokenRequest {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `name=value`, rendering `None` as Python's `str(None)`.
    #[must_use]
    pub fn field(mut self, name: &str, value: Option<&str>) -> Self {
        self.data.push((
            name.to_string(),
            value.map_or_else(|| "None".to_string(), str::to_string),
        ));
        self
    }

    /// Append a literal value (a `scope` or `grant_type` constant).
    #[must_use]
    pub fn constant(mut self, name: &str, value: &str) -> Self {
        self.data.push((name.to_string(), value.to_string()));
        self
    }

    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// `urlencode(data, doseq=True)`: the `application/x-www-form-urlencoded`
    /// body `requests` sends for a dict (`requests/models.py:464-485`).
    #[must_use]
    pub fn body(&self) -> String {
        crate::auth_flow::redirect::urlencode(&self.data)
    }
}

/// `OauthAdapter.token_data`: the normalised token dict
/// (`provider/oauth/*.py:set_token_data`).
///
/// The `Value` fields keep Python's types: a provider that puts an `int`
/// `access_token` into the dict binds `str(value)` into the `TextField`, and a
/// `None`/absent `access_token` is the swallowed `IntegrityError`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenData {
    /// `token_response.get("access_token")`.
    pub access_token: Value,
    /// `token_response.get("refresh_token", None)`.
    pub refresh_token: Value,
    /// `datetime.fromtimestamp(token_response["expires_in"], tz=utc)`, or
    /// `None` when `expires_in` is falsy.
    pub access_token_expired_at: Option<DateTime<Utc>>,
    /// The provider's `refresh_token_expired_at`, or `None`.
    pub refresh_token_expired_at: Option<DateTime<Utc>>,
    /// `token_response.get("id_token", "")`.
    pub id_token: Value,
}

impl TokenData {
    /// The normalised dict every provider builds, with `Value::Null` for an
    /// absent key — that is `dict.get`'s `None`.
    #[must_use]
    pub fn new(access_token: Value, refresh_token: Value, id_token: Value) -> Self {
        Self {
            access_token,
            refresh_token,
            access_token_expired_at: None,
            refresh_token_expired_at: None,
            id_token,
        }
    }

    /// `self.token_data.get("access_token")` as `f"Bearer {…}"` renders it
    /// (`oauth.py:88`).
    #[must_use]
    pub fn bearer(&self) -> String {
        format!("Bearer {}", python_str(&self.access_token))
    }
}

/// The provider-specific half of `OauthAdapter`: what
/// `provider/oauth/{google,github,gitlab,gitea}.py` implement.
///
/// Everything here is called in `authenticate()`'s order —
/// `token_request`, `token_data`, `user_data` — matching the Python:
/// `set_token_data()` then `set_user_data()` then `complete_login_or_signup()`.
pub trait OAuthProvider: Send + Sync {
    /// `self.provider`: `"google"`, `"github"`, `"gitlab"` or `"gitea"`.
    fn provider(&self) -> &'static str;

    /// `provider.get_auth_url()`: the redirect the initiate view answers with,
    /// including `state` and the `redirect_uri` from [`oauth_redirect_uri`].
    fn auth_url(&self) -> &str;

    /// `get_token_url()`.
    fn token_url(&self) -> &str;

    /// `get_user_info_url()`.
    fn userinfo_url(&self) -> &str;

    /// `OauthAdapter.authentication_error_code` (`oauth.py:49-59`).
    fn authentication_error_code(&self) -> AuthErrorCode;

    /// The provider's `set_token_data` request (`oauth.py:75-84`).
    fn token_request(&self, code: Option<&str>) -> TokenRequest;

    /// Map the token endpoint's JSON to [`TokenData`]. A `TypeError` here
    /// (`expires_in` as a string) is Python's uncaught 500.
    ///
    /// # Errors
    ///
    /// [`AdapterError::Internal`] for a malformed response.
    fn token_data(&self, response: &Value) -> Result<TokenData, AdapterError>;

    /// `set_user_data()`: the claim mapping, including any extra call the
    /// provider makes (`GitHub`'s `/user/emails` and organisation check,
    /// `Gitea`'s `/emails`).
    ///
    /// `response` is `self.get_user_response()` — the userinfo endpoint's JSON,
    /// already fetched by [`OauthAdapter::authenticate`] because that call is
    /// `OauthAdapter`'s own (`oauth.py:86-96`). `http` carries the bearer token
    /// and the provider's error code for the extra calls.
    fn user_data<'a>(
        &'a self,
        http: &'a OAuthHttp<'a>,
        response: &'a Value,
    ) -> ProviderFuture<'a, Value>;
}

/// The outbound calls a provider's `set_user_data` may make.
pub struct OAuthHttp<'a> {
    engine: &'a HttpEngine,
    token_data: &'a TokenData,
    error_code: AuthErrorCode,
}

impl<'a> OAuthHttp<'a> {
    /// `headers = {"Authorization": f"Bearer {self.token_data.get('access_token')}"}`.
    #[must_use]
    pub fn bearer(&self) -> String {
        self.token_data.bearer()
    }

    /// The provider's `authentication_error_code()`.
    #[must_use]
    pub fn error_code(&self) -> AuthErrorCode {
        self.error_code
    }

    /// `requests.get(url, headers=headers)` then `response.json()`, with a
    /// `RequestException` mapped to the provider's error — the shape of
    /// `get_user_token`/`get_user_response` (`oauth.py:75-96`).
    ///
    /// # Errors
    ///
    /// The provider's `*_OAUTH_PROVIDER_ERROR` for a transport failure or a
    /// 4xx/5xx; [`AdapterError::Internal`] for a body that is not JSON, which
    /// is Python's uncaught `ValueError`.
    pub async fn get_json(
        &self,
        preset: &Preset,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<Value, AdapterError> {
        let mut request = OutboundRequest::new();
        for (name, value) in headers {
            request = request.header(name.clone(), value.clone());
        }
        let (response, _) = self
            .engine
            .fetch(preset, Method::GET, &SsrfPolicy::strict(), url, &request)
            .await
            .map_err(|error| self.request_exception(&error))?;
        if !(200..300).contains(&response.status) {
            tracing::warn!(target: "plane.auth", status = response.status, "oauth request failed");
            return Err(AdapterError::Auth(AuthFlowError::new(self.error_code)));
        }
        // `response.json()` is inside the Python `try`, and requests 2.33's
        // `JSONDecodeError` subclasses `RequestException`
        // (`requests/exceptions.py`), so a non-JSON body is the provider's
        // error — not the 500 an older requests would have produced.
        parse_json(&response.text())
            .map_err(|_| AdapterError::Auth(AuthFlowError::new(self.error_code)))
    }

    /// `requests.get(url, headers=headers)` with **no** `raise_for_status`, as
    /// `GitHubOAuthProvider.is_user_in_organization` does (`github.py:139-152`).
    /// A transport failure is uncaught there, so it is a 500 here.
    ///
    /// # Errors
    ///
    /// [`AdapterError::Internal`] for a transport failure.
    pub async fn get_status(
        &self,
        preset: &Preset,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<u16, AdapterError> {
        let mut request = OutboundRequest::new();
        for (name, value) in headers {
            request = request.header(name.clone(), value.clone());
        }
        self.engine
            .fetch(preset, Method::GET, &SsrfPolicy::strict(), url, &request)
            .await
            .map(|(response, _)| response.status)
            .map_err(|error| AdapterError::Internal(error.to_string()))
    }

    /// `requests.get(url, headers=headers)` with **no** `raise_for_status`,
    /// returning the status and the decoded body.
    ///
    /// Both `__get_email` helpers handle a non-2xx themselves and catch
    /// `RequestException` with a message of their own (`github.py:104-137`,
    /// `gitea.py:117-155`), so the two halves stay separate here: a transport
    /// failure comes back as [`FetchError`] for the provider to map, and the
    /// body is returned unparsed because a non-JSON one is Python's uncaught
    /// `response.json()` `ValueError` — a 500 the provider decides about.
    ///
    /// # Errors
    ///
    /// The [`FetchError`] of the request itself.
    pub async fn get_unchecked(
        &self,
        preset: &Preset,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<UncheckedResponse, FetchError> {
        let mut request = OutboundRequest::new();
        for (name, value) in headers {
            request = request.header(name.clone(), value.clone());
        }
        let (response, _) = self
            .engine
            .fetch(preset, Method::GET, &SsrfPolicy::strict(), url, &request)
            .await?;
        Ok(UncheckedResponse {
            status: response.status,
            body: response.text(),
        })
    }

    /// The provider's `*_OAUTH_PROVIDER_ERROR` for a `requests.RequestException`
    /// — `get_user_token`/`get_user_response` catch exactly that class
    /// (`oauth.py:81-84,92-96`). A TLS configuration failure is an `OSError` in
    /// Python, which those handlers do **not** catch: it stays a 500.
    fn request_exception(&self, error: &FetchError) -> AdapterError {
        tracing::warn!(target: "plane.auth", %error, "oauth request failed");
        if error.is_request_exception() {
            AdapterError::Auth(AuthFlowError::new(self.error_code))
        } else {
            AdapterError::Internal(error.to_string())
        }
    }
}

/// `response.json()`: `json.loads(response.text)`.
///
/// `requests` decodes the body with the header's charset (`utf-8` for
/// `application/json`, `ISO-8859-1` for `text/*`) and replaces invalid bytes;
/// [`plane_net::PinnedResponse::text`] is that decoder.
pub(crate) fn parse_json(body: &str) -> Result<Value, AdapterError> {
    serde_json::from_str(body)
        .map_err(|error| AdapterError::Internal(format!("json.decoder.JSONDecodeError: {error}")))
}

/// A response fetched without `raise_for_status`: what
/// [`OAuthHttp::get_unchecked`] hands a provider's `__get_email`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncheckedResponse {
    pub status: u16,
    pub body: String,
}

impl UncheckedResponse {
    /// `response.ok` (`requests`' `status_code < 400`).
    #[must_use]
    pub fn ok(&self) -> bool {
        self.status < 400
    }

    /// `response.json()`, with the decode failure left to the provider: both
    /// `__get_email` helpers call it inside their `except RequestException`
    /// block, and requests 2.33's `JSONDecodeError` **is** a
    /// `RequestException` (`requests/exceptions.py`).
    ///
    /// # Errors
    ///
    /// [`serde_json::Error`] for a body that is not JSON.
    pub fn json(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_str(&self.body)
    }
}

/// `OauthAdapter` (`adapter/oauth.py:24-132`) with an [`OAuthProvider`] behind
/// it.
pub struct OauthAdapter {
    provider: Box<dyn OAuthProvider>,
    code: Option<String>,
    token_data: Option<TokenData>,
    user_data: Value,
    /// `check_sync_enabled()`, resolved by the caller (`AUTH_FLOWS.md` §2i).
    sync_enabled: bool,
}

impl OauthAdapter {
    /// Build the adapter for a callback. `sync_enabled` is
    /// `get_configuration_value([{"key": sync_flag_name(provider), "default":
    /// os.environ.get(key, "0")}])[0] == "1"`.
    #[must_use]
    pub fn new(provider: Box<dyn OAuthProvider>, code: Option<String>, sync_enabled: bool) -> Self {
        Self {
            provider,
            code,
            token_data: None,
            user_data: Value::Null,
            sync_enabled,
        }
    }

    /// The provider, for `get_auth_url()` and the initiate view.
    #[must_use]
    pub fn provider(&self) -> &dyn OAuthProvider {
        self.provider.as_ref()
    }

    /// `self.get_auth_url()`.
    #[must_use]
    pub fn auth_url(&self) -> &str {
        self.provider.auth_url()
    }

    /// `OauthAdapter.authenticate` (`oauth.py:70-73`):
    /// `set_token_data()`, `set_user_data()`, `complete_login_or_signup()`.
    ///
    /// # Errors
    ///
    /// Every `AuthenticationException` the exchange or the login raises, plus
    /// the 500s described on [`OAuthProvider`].
    pub async fn authenticate(
        &mut self,
        context: &AdapterContext<'_>,
        callback: Option<&dyn AuthCallback>,
        enable_signup: bool,
    ) -> Result<CompletedLogin, AdapterError> {
        let token_data = self.exchange_token().await?;
        let http = OAuthHttp {
            engine: engine(),
            token_data: &token_data,
            error_code: self.provider.authentication_error_code(),
        };
        // `get_user_response()` (`oauth.py:86-96`): GET the userinfo endpoint
        // with the bearer token; a `RequestException` or a 4xx/5xx is the
        // provider's error, a non-JSON body is a 500.
        let headers = [("Authorization".to_string(), http.bearer())];
        let response = http
            .get_json(
                &preset::OAUTH_USERINFO,
                self.provider.userinfo_url(),
                &headers,
            )
            .await
            .map_err(|error| {
                tracing::warn!(target: "plane.auth", error = ?error, "Error getting user response");
                error
            })?;
        self.user_data = self.provider.user_data(&http, &response).await?;
        self.token_data = Some(token_data);
        complete_login_or_signup(context, self, callback, enable_signup).await
    }

    /// `set_token_data()`'s HTTP half: POST the provider's form to
    /// `get_token_url()` and map the reply to [`TokenData`].
    async fn exchange_token(&mut self) -> Result<TokenData, AdapterError> {
        let request = self.provider.token_request(self.code.as_deref());
        let mut outbound = OutboundRequest::new()
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(request.body().into_bytes());
        for (name, value) in &request.headers {
            outbound = outbound.header(name.clone(), value.clone());
        }
        let (response, _) = engine()
            .fetch(
                &preset::OAUTH_TOKEN,
                Method::POST,
                &SsrfPolicy::strict(),
                self.provider.token_url(),
                &outbound,
            )
            .await
            .map_err(|error| {
                tracing::warn!(target: "plane.auth", %error, "Error getting user token");
                if error.is_request_exception() {
                    self.token_error()
                } else {
                    AdapterError::Internal(error.to_string())
                }
            })?;
        if !(200..300).contains(&response.status) {
            tracing::warn!(target: "plane.auth", status = response.status, "Error getting user token");
            return Err(self.token_error());
        }
        // As in `get_json`: `response.json()`'s decode error is a
        // `RequestException` in requests 2.33 and lands in `get_user_token`'s
        // `except` (`oauth.py:78-85`).
        let body = parse_json(&response.text()).map_err(|_| self.token_error())?;
        self.provider.token_data(&body)
    }

    /// The provider's `*_OAUTH_PROVIDER_ERROR` (`oauth.py:81-84`).
    fn token_error(&self) -> AdapterError {
        AdapterError::Auth(AuthFlowError::new(
            self.provider.authentication_error_code(),
        ))
    }
}

/// The process-wide outbound client (`DECISIONS.md` D9).
fn engine() -> &'static HttpEngine {
    static ENGINE: std::sync::OnceLock<HttpEngine> = std::sync::OnceLock::new();
    ENGINE.get_or_init(HttpEngine::system)
}

impl AuthProvider for OauthAdapter {
    fn provider(&self) -> &'static str {
        self.provider.provider()
    }

    fn user_data(&self) -> &Value {
        &self.user_data
    }

    fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    fn has_token_data(&self) -> bool {
        // `if self.token_data:` — set after a successful exchange.
        self.token_data.is_some()
    }

    fn sync_enabled(&self) -> bool {
        self.sync_enabled
    }

    fn sync_user_data<'a>(
        &'a self,
        context: &'a AdapterContext<'a>,
        user: &'a mut User,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let profile = self.user_data.get("user");
            let first_name = profile
                .and_then(|profile| profile.get("first_name"))
                .filter(|value| truthy(Some(value)))
                .map(python_str)
                .unwrap_or_default();
            let last_name = profile
                .and_then(|profile| profile.get("last_name"))
                .filter(|value| truthy(Some(value)))
                .map(python_str)
                .unwrap_or_default();
            user.first_name = first_name;
            user.last_name = last_name;

            let email = self.user_data.get("email").cloned().unwrap_or(Value::Null);
            let display_name = profile
                .and_then(|profile| profile.get("display_name"))
                .filter(|value| truthy(Some(value)))
                .map(python_str)
                .unwrap_or_else(|| display_name_from_email(&email));
            user.display_name = display_name;

            let avatar = profile
                .and_then(|profile| profile.get("avatar"))
                .filter(|value| truthy(Some(value)))
                .map(python_str)
                .unwrap_or_default();
            delete_old_avatar(context, user).await;
            match upload_avatar(self.provider.provider(), context, &avatar, user.id).await {
                Some(asset) => user.avatar_asset_id = Some(asset),
                None => user.avatar = avatar,
            }
            Ok(())
        })
    }

    fn upload_signup_avatar<'a>(
        &'a self,
        context: &'a AdapterContext<'a>,
        user: &'a mut User,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let avatar = self
                .user_data
                .get("user")
                .and_then(|profile| profile.get("avatar"))
                .filter(|value| truthy(Some(value)))
                .map(python_str)
                .unwrap_or_default();
            if avatar.is_empty() {
                return Ok(());
            }
            match upload_avatar(self.provider.provider(), context, &avatar, user.id).await {
                Some(asset) => {
                    user.avatar_asset_id = Some(asset);
                    user.avatar = avatar;
                }
                None => user.avatar = avatar,
            }
            Ok(())
        })
    }

    fn create_update_account<'a>(
        &'a self,
        client: &'a Client,
        user: &'a User,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let token_data = self.token_data.as_ref().ok_or_else(|| {
                AdapterError::Internal("create_update_account without token_data".to_string())
            })?;
            create_update_account(
                client,
                self.provider.provider(),
                &self.user_data,
                token_data,
                user.id,
            )
            .await
        })
    }
}

/// `User.get_display_name(email)` (`plane/db/models/user.py:189-197`).
///
/// `email.split("@")[0]` only when there are exactly two parts; otherwise six
/// random ASCII letters. An empty/`null` email is the random branch.
fn display_name_from_email(email: &Value) -> String {
    let Some(email) = email.as_str() else {
        return random_letters();
    };
    if email.is_empty() {
        return random_letters();
    }
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() == 2 {
        parts[0].to_string()
    } else {
        random_letters()
    }
}

/// `"".join(random.choice(string.ascii_letters) for _ in range(6))`
/// (`user.py:192,196`).
fn random_letters() -> String {
    get_random_string(6, ASCII_LETTERS)
}

/// `create_update_account(user=user)` (`adapter/oauth.py:101-132`).
///
/// `Account.objects.filter(user=…, provider=…, provider_account_id=…).first()`
/// — the model's `Meta.ordering = ("-created_at",)` decides which row wins —
/// then a full `save()` or a `create()`.
///
/// The `except (DatabaseError, IntegrityError): log_exception(e)` is ported as
/// "swallow a Postgres **error response**": a transport failure is an
/// `OperationalError` in psycopg, which Python does not catch, so it stays a
/// 500 here.
///
/// # Errors
///
/// [`AdapterError::Internal`] when `user_data` has no `"user"` object (an
/// `AttributeError` in Python, outside the `try`) or the connection fails.
pub async fn create_update_account(
    client: &Client,
    provider: &str,
    user_data: &Value,
    token_data: &TokenData,
    user_id: Uuid,
) -> Result<(), AdapterError> {
    let profile = match user_data.get("user") {
        Some(Value::Object(profile)) => profile,
        Some(other) => {
            return Err(AdapterError::Internal(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                value_type_name(other)
            )));
        }
        None => {
            return Err(AdapterError::Internal(
                "AttributeError: 'NoneType' object has no attribute 'get'".to_string(),
            ));
        }
    };
    // `self.user_data.get("user").get("provider_id")`.
    let provider_account_id = text_bind(profile.get("provider_id").unwrap_or(&Value::Null));
    let provider = provider.to_string();

    let statement = crate::queries::ACCOUNT_BY_USER_PROVIDER_ID
        .prepare(client)
        .await?;
    let existing = client
        .query_opt(&statement, &[&user_id, &provider, &provider_account_id])
        .await?;

    // Every `timezone.now()` in the Python is a separate instant (`auto_now_add`
    // and `auto_now` each call it, and `last_connected_at` is assigned before
    // `save()`), so they are separate here too.
    let now = || Utc::now().trunc_subsecs(6);
    let access_token = text_bind(&token_data.access_token);
    let refresh_token = text_bind(&token_data.refresh_token);
    let id_token = text_bind(&token_data.id_token);

    let outcome = match &existing {
        Some(row) => {
            let created_at: DateTime<Utc> = row.get(0);
            let metadata: JsonText = row.get(12);
            let updated_at = now();
            let last_connected_at = now();
            let statement = crate::queries::UPDATE_ACCOUNT.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &created_at,
                        &updated_at,
                        &user_id,
                        &provider_account_id,
                        &provider,
                        &access_token,
                        &token_data.access_token_expired_at,
                        &refresh_token,
                        &token_data.refresh_token_expired_at,
                        &last_connected_at,
                        &id_token,
                        &metadata,
                        &row.get::<_, Uuid>(2),
                    ],
                )
                .await
        }
        None => {
            let created_at = now();
            let updated_at = now();
            let last_connected_at = now();
            let statement = crate::queries::INSERT_ACCOUNT.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &created_at,
                        &updated_at,
                        &Uuid::new_v4(),
                        &user_id,
                        &provider_account_id,
                        &provider,
                        &access_token,
                        &token_data.access_token_expired_at,
                        &refresh_token,
                        &token_data.refresh_token_expired_at,
                        &last_connected_at,
                        &id_token,
                        &JsonText::new("{}"),
                    ],
                )
                .await
        }
    };

    match outcome {
        Ok(_) => Ok(()),
        // `except (DatabaseError, IntegrityError) as e: log_exception(e)`.
        Err(error) if error.as_db_error().is_some() => {
            tracing::warn!(target: "plane.auth", %error, "create_update_account failed");
            Ok(())
        }
        Err(error) => Err(AdapterError::Internal(error.to_string())),
    }
}

/// `type(value).__name__` for the `user_data` values `python_str` can see.
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

/// `str(value)` for a `TextField`/`CharField` bind: `None` for `null`, Python's
/// `str()` for everything else (`TextField.get_prep_value`).
///
/// A container is rendered with `json.dumps` here rather than Python's `repr`;
/// no provider puts one in a token.
fn text_bind(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        // `str({...})` with single quotes is Python's `repr`; JSON is the
        // closest stable rendering and unreachable for the four providers.
        Value::Array(_) | Value::Object(_) => Some(value.to_string()),
        other => Some(python_str(other)),
    }
}

/// `sync_user_data`'s `delete_old_avatar` (`base.py:253-272`).
///
/// Three Python branches all end with `avatar_asset = None`, but only the
/// "asset found and storage answered" path also clears `avatar`; a storage
/// construction failure returns **before** touching either field, so the old
/// asset stays linked (and the caller's later `save()` persists it).
async fn delete_old_avatar(context: &AdapterContext<'_>, user: &mut User) {
    let Some(asset_id) = user.avatar_asset_id else {
        return;
    };
    // `FileAsset.objects.get(pk=…, user_id=…)`: the soft-delete manager.
    // `S3Storage(request=…)` is constructed only after that lookup succeeds
    // (`base.py:256-258`), so a broken storage configuration does not stop the
    // link from being dropped.
    let key = match load_asset_key(context, asset_id, user.id).await {
        Ok(Some(key)) => key,
        // `FileAsset.DoesNotExist`: drop the link without touching the asset.
        Ok(None) => {
            user.avatar_asset_id = None;
            return;
        }
        Err(error) => {
            log_avatar_failure(&error);
            return;
        }
    };
    let Ok(storage) = Storage::from_process_env(&context.settings.storage) else {
        tracing::warn!(target: "plane.auth", "object storage is not configured");
        return;
    };
    let request = storage.request(storage_request_info(context.ctx));
    // `storage.delete_files([asset.asset.name])` catches only boto3's
    // `ClientError`; a **transport** failure (`EndpointConnectionError` — an
    // unreachable custom domain) propagates to `delete_old_avatar`'s
    // `except Exception`, which returns before dropping the link
    // (`base.py:253-272`). `plane-storage` reports an S3 error response as
    // `false` and a transport failure as `Err` (DECISIONS.md D6), so the two
    // branches are distinguishable here.
    match request.delete_objects(&[key.as_str()]).await {
        Ok(_) => {}
        Err(error) => {
            log_avatar_failure(&error);
            return;
        }
    }
    let deleted: Result<u64, plane_db::DbError> = async {
        let statement = crate::queries::DELETE_FILE_ASSET_BY_ID
            .prepare(context.client)
            .await?;
        Ok(context.client.execute(&statement, &[&asset_id]).await?)
    }
    .await;
    match deleted {
        Ok(_) => {
            user.avatar_asset_id = None;
            user.avatar = String::new();
        }
        Err(error) => log_avatar_failure(&error),
    }
}

/// `FileAsset.objects.get(pk=…, user_id=…).asset.name`.
async fn load_asset_key(
    context: &AdapterContext<'_>,
    asset_id: Uuid,
    user_id: Uuid,
) -> Result<Option<String>, plane_db::DbError> {
    let statement = crate::queries::FILE_ASSET_OBJECT_KEY
        .prepare(context.client)
        .await?;
    let row = context
        .client
        .query_opt(&statement, &[&asset_id, &user_id])
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// `download_and_upload_avatar` (`base.py:136-229`), reduced to the new
/// `FileAsset`'s id.
///
/// Every failure is `None`: the caller falls back to the provider's URL. The
/// body is fetched through the pinned [`preset::OAUTH_AVATAR`] client, whose
/// `DATA_UPLOAD_MAX_MEMORY_SIZE` cap is Python's `stream=True` size check.
async fn upload_avatar(
    provider: &str,
    context: &AdapterContext<'_>,
    avatar_url: &str,
    user_id: Uuid,
) -> Option<Uuid> {
    if avatar_url.is_empty() {
        return None;
    }
    let max_size = context.settings.limits.data_upload_max_memory_size;
    let response = match engine()
        .fetch(
            &preset::OAUTH_AVATAR,
            Method::GET,
            &SsrfPolicy::strict(),
            avatar_url,
            &OutboundRequest::new().max_body_bytes(usize::try_from(max_size).unwrap_or(usize::MAX)),
        )
        .await
    {
        Ok((response, _)) => response,
        Err(error) => {
            log_avatar_failure(&error);
            return None;
        }
    };
    // `response.raise_for_status()`.
    if !(200..300).contains(&response.status) {
        tracing::warn!(target: "plane.auth", status = response.status, "avatar download failed");
        return None;
    }
    // `if content_length and int(content_length) > max_size: return None`, and
    // an unparseable length is `int()`'s `ValueError` — also `None`.
    if let Some(length) = response.header("content-length") {
        match plane_core::py::int(length) {
            Ok(length) if length > max_size => return None,
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    let content_type = response.header("content-type").unwrap_or("image/jpeg");
    let extension = match content_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    let content = response.bytes();
    let file_size = content.len() as i64;
    let filename = format!("{}-user-avatar.{extension}", Uuid::new_v4().simple());
    let storage = match Storage::from_process_env(&context.settings.storage) {
        Ok(storage) => storage,
        Err(error) => {
            log_avatar_failure(&error);
            return None;
        }
    };
    let request = storage.request(storage_request_info(context.ctx));
    match request
        .upload_file(&filename, content, Some(content_type))
        .await
    {
        Ok(true) => {}
        Ok(false) => return None,
        Err(error) => {
            log_avatar_failure(&error);
            return None;
        }
    }
    let metadata = match request.head_object(&filename).await {
        Ok(metadata) => metadata,
        Err(error) => {
            log_avatar_failure(&error);
            return None;
        }
    };
    let attributes = serde_json::json!({
        "name": format!("{provider}-avatar.{extension}"),
        "type": content_type,
        "size": file_size,
    });
    let attributes = json_text(&attributes)?;
    // `storage_metadata=None` is a SQL NULL; the dict is `json.dumps`-encoded.
    let metadata = match metadata {
        Some(metadata) => Some(json_text(&metadata)?),
        None => None,
    };
    let created_at = Utc::now().trunc_subsecs(6);
    let updated_at = Utc::now().trunc_subsecs(6);
    let statement = crate::queries::INSERT_USER_AVATAR_ASSET
        .prepare(context.client)
        .await
        .map_err(|error| log_avatar_failure(&error))
        .ok()?;
    let asset_id = Uuid::new_v4();
    let result = context
        .client
        .execute(
            &statement,
            &[
                &created_at,
                &updated_at,
                &asset_id,
                &attributes,
                &filename,
                &Some(user_id),
                &None::<Uuid>,
                &None::<Uuid>,
                &false,
                &None::<DateTime<Utc>>,
                &false,
                &None::<Uuid>,
                &Some("USER_AVATAR"),
                &None::<String>,
                &None::<String>,
                &true,
                &None::<Uuid>,
                &None::<Uuid>,
                &None::<Uuid>,
                &(file_size as f64),
                &metadata,
                &Some(user_id),
                &None::<Uuid>,
                &None::<String>,
            ],
        )
        .await;
    match result {
        Ok(_) => Some(asset_id),
        Err(error) => {
            log_avatar_failure(&error);
            None
        }
    }
}

/// `json.dumps(value, cls=DjangoJSONEncoder)` as the `JSONField` text.
fn json_text<T: serde::Serialize + ?Sized>(value: &T) -> Option<JsonText> {
    let bytes = plane_serde::to_vec(Dialect::Django, value).map_err(|error| {
        log_avatar_failure(&error);
    });
    let bytes = bytes.ok()?;
    match String::from_utf8(bytes) {
        Ok(text) => Some(JsonText::new(text)),
        Err(error) => {
            log_avatar_failure(&error);
            None
        }
    }
}

/// `log_exception` on the avatar path: Python logs and continues.
fn log_avatar_failure(error: &dyn std::fmt::Display) {
    tracing::warn!(target: "plane.auth", %error, "avatar download or upload failed");
}

/// `S3Storage(request=self.request)`'s `RequestInfo`.
fn storage_request_info(ctx: &RequestContext) -> RequestInfo<'_> {
    RequestInfo {
        scheme: if ctx.is_secure { "https" } else { "http" },
        host: ctx.header("host").unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(value: &str) -> Json {
        Json::Str(value.to_string())
    }

    #[test]
    fn the_state_check_is_the_python_expression() {
        let stored = state("0123456789abcdef0123456789abcdef");
        assert!(
            oauth_state_matches(Some("0123456789abcdef0123456789abcdef"), Some(&stored)).unwrap()
        );
        assert!(!oauth_state_matches(Some("other"), Some(&stored)).unwrap());
        // A missing/empty query parameter, a missing stored state, and the
        // falsy stored values.
        assert!(!oauth_state_matches(None, Some(&stored)).unwrap());
        assert!(!oauth_state_matches(Some(""), Some(&stored)).unwrap());
        assert!(!oauth_state_matches(Some("x"), None).unwrap());
        assert!(!oauth_state_matches(Some("x"), Some(&Json::Null)).unwrap());
        assert!(!oauth_state_matches(Some("x"), Some(&state(""))).unwrap());
        assert!(!oauth_state_matches(Some("x"), Some(&Json::Int(0))).unwrap());
        assert!(!oauth_state_matches(Some("x"), Some(&Json::Bool(false))).unwrap());
        // Different lengths still compare (constant time over the shorter).
        assert!(!oauth_state_matches(Some("ab"), Some(&state("abc"))).unwrap());
    }

    #[test]
    fn a_non_string_state_is_the_python_attribute_error() {
        let error = oauth_state_matches(Some("x"), Some(&Json::Int(5))).expect_err("int state");
        let AdapterError::Internal(message) = error else {
            panic!("not internal: {error:?}");
        };
        assert_eq!(
            message,
            "AttributeError: 'int' object has no attribute 'encode'"
        );
    }

    #[test]
    fn the_redirect_uri_is_built_from_host_and_scheme() {
        let mut ctx = RequestContext::new(
            http::Method::GET,
            "/auth/google/",
            "",
            http::HeaderMap::new(),
            None,
        );
        ctx.headers
            .insert("host", "app.plane.so:8080".parse().unwrap());
        assert_eq!(
            oauth_redirect_uri(&ctx, "google"),
            "http://app.plane.so:8080/auth/google/callback/"
        );
        ctx.is_secure = true;
        assert_eq!(
            oauth_redirect_uri(&ctx, "gitea"),
            "https://app.plane.so:8080/auth/gitea/callback/"
        );
    }

    #[test]
    fn the_sync_flag_map_is_the_python_dict() {
        assert_eq!(sync_flag_name("google"), Some("ENABLE_GOOGLE_SYNC"));
        assert_eq!(sync_flag_name("github"), Some("ENABLE_GITHUB_SYNC"));
        assert_eq!(sync_flag_name("gitlab"), Some("ENABLE_GITLAB_SYNC"));
        assert_eq!(sync_flag_name("gitea"), Some("ENABLE_GITEA_SYNC"));
        assert_eq!(sync_flag_name("email"), None);
    }

    #[test]
    fn the_token_form_is_urlencoded_like_requests() {
        let request = TokenRequest::new()
            .field("code", Some("a b+c"))
            .field("client_secret", None)
            .constant("grant_type", "authorization_code")
            .header("Accept", "application/json");
        assert_eq!(
            request.body(),
            "code=a+b%2Bc&client_secret=None&grant_type=authorization_code"
        );
        assert_eq!(
            request.headers,
            vec![("Accept".into(), "application/json".into())]
        );
    }

    #[test]
    fn the_bearer_header_renders_str_none() {
        let mut token_data = TokenData::new(json!("abc"), Value::Null, json!(""));
        assert_eq!(token_data.bearer(), "Bearer abc");
        token_data.access_token = Value::Null;
        assert_eq!(token_data.bearer(), "Bearer None");
    }

    #[test]
    fn the_display_name_follows_get_display_name() {
        assert_eq!(
            display_name_from_email(&json!("first.last@x.com")),
            "first.last"
        );
        // Not exactly two parts: random six letters.
        assert_eq!(display_name_from_email(&json!("a@b@c")).len(), 6);
        assert_eq!(display_name_from_email(&json!("nodomain")).len(), 6);
        assert_eq!(display_name_from_email(&Value::Null).len(), 6);
        // `"@x.com".split("@")` is `["", "x.com"]` — two parts, so the empty
        // local part is the display name.
        assert_eq!(display_name_from_email(&json!("@x.com")), "");
    }

    #[test]
    fn text_binds_follow_get_prep_value() {
        assert_eq!(text_bind(&json!("abc")), Some("abc".to_string()));
        assert_eq!(text_bind(&json!(5)), Some("5".to_string()));
        assert_eq!(text_bind(&json!(true)), Some("True".to_string()));
        assert_eq!(text_bind(&Value::Null), None);
    }
}
