// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Pieces every `/auth/` session handler shares (`AUTH_FLOWS.md` set A).
//!
//! The Python views are thin: each one reads `request.POST`/`request.data`,
//! applies an instance-configuration gate, calls the credential adapter and
//! builds one redirect. The differences between the app and space variants are
//! small but load-bearing (`AUTH_FLOWS.md` §5), so they stay visible in the
//! handlers and only the mechanical parts live here:
//!
//! | Helper | Python |
//! | --- | --- |
//! | [`base_host`] | `authentication/utils/host.py:16-63` (`view_base_host`) |
//! | [`post_params`] | `request.POST` (`django/http/request.py:529-571`) |
//! | [`data_get`] | `request.data.get` for the DRF views |
//! | [`redirect_error`], [`redirect_url`] | `HttpResponseRedirect` over `get_safe_redirect_url` |
//! | [`throttle_redirect`] | `@throttle_auth_redirect` (`rate_limit.py:62-95`) |
//! | [`email_authenticate`] | `EmailProvider` + `Adapter.complete_login_or_signup` |
//! | [`redirection_path`] | `authentication/utils/redirection_path.py` |
//! | [`user_login`] | `authentication/utils/login.py:13-27` |

use chrono::{SubsecRound, Utc};
use deadpool_postgres::Client;
use plane_auth::auth_flow::adapter::{
    AdapterContext, AdapterError, AuthCallback, AuthProvider, CompletedLogin, PostUserAuthWorkflow,
    complete_login_or_signup,
};
use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::instance;
use plane_auth::auth_flow::redirect::{
    PyValueError, get_safe_redirect_url, settings_allowed_hosts, space_redirect_url,
};
use plane_auth::auth_flow::tasks::view_base_host;
use plane_auth::{LoginFlags, check_password_async, user_login as django_user_login};
use plane_core::error::{APPLICATION_JSON, TEXT_HTML};
use plane_core::{Object, Settings};
use plane_db::JsonText;
use plane_db::models::user::{Profile, User};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_throttle::{ThrottleClass, ThrottleOutcome, ThrottleRequest, check_throttles};
use serde_json::Value;

use super::queries;

/// The body parsers Django's `request.POST` accepts (`request.py:551-571`):
/// `application/x-www-form-urlencoded` and `multipart/form-data`. A JSON or
/// text body leaves `POST` empty, which is why these views ignore `request.data`.
fn post_parsers() -> [String; 2] {
    [
        "rest_framework.parsers.FormParser".to_string(),
        "rest_framework.parsers.MultiPartParser".to_string(),
    ]
}

/// `timezone.now()` at the precision Postgres stores (matching the adapters).
fn now() -> chrono::DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

/// `plane/authentication/utils/host.py:16-63` (`base_host`), with the `None`
/// result turned into the 500 Python's `None + path`/`urljoin` would produce.
///
/// The view copy returns `None` for `is_app=True` when neither `WEB_URL` nor
/// `APP_BASE_URL` is set; `get_safe_redirect_url` then calls
/// `base_url.rstrip("/")` on `None` → `AttributeError` → 500.
///
/// # Errors
///
/// [`ViewError::Unknown`] when the base host is `None` or the space branch's
/// concatenation raises.
pub fn base_host(settings: &Settings, flags: LoginFlags) -> Result<String, ViewError> {
    match view_base_host(settings, flags) {
        Ok(Some(host)) => Ok(host),
        Ok(None) => Err(ViewError::Unknown(
            "AttributeError: 'NoneType' object has no attribute 'rstrip'".to_string(),
        )),
        Err(error) => Err(ViewError::Unknown(error.to_string())),
    }
}

/// `get_allowed_hosts()` (`path_validator.py:91-102`).
fn allowed_hosts(settings: &Settings) -> Result<Vec<String>, ViewError> {
    settings_allowed_hosts(settings).map_err(py_value_error)
}

fn py_value_error(error: PyValueError) -> ViewError {
    ViewError::Unknown(format!("ValueError: {}", error.message()))
}

/// `HttpResponseRedirect(url)`: 302, an empty body and `text/html`.
pub fn redirect_url(location: impl Into<String>) -> ViewResponse {
    ViewResponse::raw(302, TEXT_HTML, Vec::new()).with_header("Location", location)
}

/// `get_safe_redirect_url(base_url=base_host(...), next_path=..., params=...)`
/// followed by `HttpResponseRedirect`.
///
/// # Errors
///
/// [`ViewError::Unknown`] for the `ValueError`s the Python view does not catch
/// (a malformed `next_path`/base URL).
pub fn redirect_error(
    ctx: &ViewCtx<'_>,
    flags: LoginFlags,
    next_path: Option<&str>,
    error: &AuthFlowError,
) -> Result<ViewResponse, ViewError> {
    let base = base_host(ctx.settings, flags)?;
    let hosts = allowed_hosts(ctx.settings)?;
    let url = get_safe_redirect_url(&base, next_path, &error.redirect_params(), &hosts)
        .map_err(py_value_error)?;
    Ok(redirect_url(url))
}

/// The success redirect of the app views: `get_safe_redirect_url(..., params={})`
/// (`views/app/email.py:124-129`).
///
/// # Errors
///
/// See [`redirect_error`].
pub fn app_success_redirect(
    ctx: &ViewCtx<'_>,
    flags: LoginFlags,
    path: Option<&str>,
) -> Result<ViewResponse, ViewError> {
    let base = base_host(ctx.settings, flags)?;
    let hosts = allowed_hosts(ctx.settings)?;
    let url = get_safe_redirect_url(&base, path, &[], &hosts).map_err(py_value_error)?;
    Ok(redirect_url(url))
}

/// The success redirect of the space views (`views/space/email.py:98-103`):
/// `base.rstrip("/") + validate_next_path(next_path)`, checked with
/// `url_has_allowed_host_and_scheme`; on failure the bare base.
///
/// # Errors
///
/// See [`redirect_error`].
pub fn space_success_redirect(
    ctx: &ViewCtx<'_>,
    flags: LoginFlags,
    next_path: Option<&str>,
) -> Result<ViewResponse, ViewError> {
    let base = base_host(ctx.settings, flags)?;
    let hosts = allowed_hosts(ctx.settings)?;
    let url = space_redirect_url(&base, next_path, &hosts).map_err(py_value_error)?;
    Ok(redirect_url(url))
}

/// `request.POST` for a plain `django.views.View`.
///
/// Django parses the body lazily on the first `request.POST` access and only
/// for the two form media types (`request.py:551-571`); every other media type
/// leaves an empty `QueryDict`. DRF's `parsers::parse` is reused for the two
/// accepted types, including `TooManyFieldsSent` and the multipart field
/// handling.
///
/// # Errors
///
/// [`ViewError::TooManyFields`] for `TooManyFieldsSent`.
pub fn post_params(ctx: &ViewCtx<'_>) -> Result<plane_drf::QueryDict, ViewError> {
    if ctx.method() != http::Method::POST {
        return Ok(plane_drf::QueryDict::default());
    }
    let parsers = post_parsers();
    match plane_drf::parsers::parse(ctx.ctx, &parsers) {
        Ok(Data::Form(query)) => Ok(query),
        // An empty body of any media type, or a JSON body: Django's `POST` is
        // empty because the parser only handles the two form media types.
        Ok(_) => Ok(plane_drf::QueryDict::default()),
        Err(ViewError::Api(plane_core::ApiError::UnsupportedMediaType(_))) => {
            Ok(plane_drf::QueryDict::default())
        }
        Err(error) => Err(error),
    }
}

/// `request.data.get(key)` for the DRF views.
///
/// `None` is Python's missing key *and* a JSON `null`; the call sites pass
/// their own default (`False` or `None`) and test truthiness, so the two are
/// equivalent there.
///
/// # Errors
///
/// [`ViewError::Unknown`] when `request.data` is a JSON list or scalar: Python's
/// `request.data.get` raises `AttributeError` on those.
pub fn data_get(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<Value>, ViewError> {
    match ctx.data()? {
        Data::Form(query) => Ok(query.get(key).map(|value| Value::String(value.to_string()))),
        Data::Json(value) => match value {
            plane_serde::pyjson::Value::Dict(dict) => Ok(dict.get(key).map(pyjson_to_serde)),
            _ => Err(ViewError::Unknown(
                "AttributeError: 'str' object has no attribute 'get'".to_string(),
            )),
        },
    }
}

/// `json.dumps`-compatible conversion of a `request.data` value.
fn pyjson_to_serde(value: &plane_serde::pyjson::Value) -> Value {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => Value::Null,
        Py::Bool(value) => Value::Bool(*value),
        Py::Int(value) => match value.as_i64() {
            Some(number) => Value::from(number),
            // A Python int bigger than i64: keep its decimal text so `str()`
            // is unchanged.
            None => Value::String(value.to_decimal_string()),
        },
        Py::Float(value) => serde_json::Number::from_f64(*value).map_or(Value::Null, Value::Number),
        Py::Str(text) => Value::String(text.to_string_lossy().into_owned()),
        Py::List(items) => Value::Array(items.iter().map(pyjson_to_serde).collect()),
        Py::Dict(dict) => {
            let mut map = serde_json::Map::new();
            for (key, value) in dict.iter() {
                map.insert(key.to_string_lossy().into_owned(), pyjson_to_serde(value));
            }
            Value::Object(map)
        }
    }
}

/// Python truthiness for the values `request.data` can hold.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// Python's `str()` for the scalar types `request.data` can hold.
pub fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// A DRF `Response(object, status=…)`, rendered by `JSONRenderer`.
///
/// # Errors
///
/// A rendering failure, which Python would raise out of the renderer (500).
pub fn json_object(status: u16, object: &Object) -> Result<ViewResponse, ViewError> {
    let body = plane_serde::to_vec(plane_serde::Dialect::Drf, object)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `@throttle_auth_redirect(...)` (`rate_limit.py:62-95`).
///
/// Runs `AuthenticationThrottle` before the view body; on refusal returns the
/// app/space redirect carrying `error_code=5900`. The throttle is the same
/// scope and key as the DRF views' (`throttle_authentication_<ip>`), so a
/// caller's budget is shared across every `/auth/` route, signed in or not.
///
/// `ctx.user` is the anonymous placeholder on a plain view; that is fine
/// because `AuthenticationThrottle.get_cache_key` keys on the client IP only
/// (`rate_limit.py:32-34`) and Python's `allow_request` never reads
/// `request.user` either.
///
/// # Errors
///
/// A cache failure is the 500 django-redis produces.
pub async fn throttle_redirect(
    ctx: &ViewCtx<'_>,
    next_path: Option<&str>,
    flags: LoginFlags,
) -> Result<Option<ViewResponse>, ViewError> {
    let redis = ctx.redis.ok_or_else(|| {
        ViewError::Unknown(
            "the process has no Redis cache handle; the auth throttle would fail".to_string(),
        )
    })?;
    let remote_addr = ctx.ctx.remote_addr();
    let request = ThrottleRequest {
        settings: &ctx.settings.throttling,
        user: ctx.user,
        headers: ctx.headers(),
        remote_addr: remote_addr.as_deref(),
        asset_id: None,
        scope_override: None,
    };
    let outcome = check_throttles(
        redis,
        &[ThrottleClass::Authentication],
        &request,
        plane_throttle::rate::now(),
    )
    .await?;
    match outcome {
        ThrottleOutcome::Allowed { .. } => Ok(None),
        ThrottleOutcome::Throttled(_) => {
            let error = AuthFlowError::new(AuthErrorCode::RateLimitExceeded);
            Ok(Some(redirect_error(ctx, flags, next_path, &error)?))
        }
    }
}

/// `User.objects.get(pk=…)` — the full row.
///
/// # Errors
///
/// A database failure.
pub async fn load_user_by_id(
    client: &Client,
    user_id: uuid::Uuid,
) -> Result<Option<User>, ViewError> {
    let statement = queries::AUTH_USER_BY_ID.prepare(client).await?;
    let row = client.query_opt(&statement, &[&user_id]).await?;
    match row {
        Some(row) => Ok(Some(User::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `User.objects.filter(email=…).first()` — the full row, an exact match.
///
/// # Errors
///
/// A database failure.
pub async fn load_user_by_email(client: &Client, email: &str) -> Result<Option<User>, ViewError> {
    let statement = queries::AUTH_USER_BY_EMAIL.prepare(client).await?;
    let row = client.query_opt(&statement, &[&email]).await?;
    match row {
        Some(row) => Ok(Some(User::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `Profile.objects.get(user=user)`.
///
/// # Errors
///
/// A database failure.
pub async fn load_profile(
    client: &Client,
    user_id: uuid::Uuid,
) -> Result<Option<Profile>, ViewError> {
    let statement = queries::AUTH_PROFILE_BY_USER.prepare(client).await?;
    let row = client.query_opt(&statement, &[&user_id]).await?;
    match row {
        Some(row) => Ok(Some(Profile::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// `Profile.objects.get_or_create(user=user)` (`redirection_path.py:10`).
///
/// The create branch is unreachable for a user who signed up through these
/// views (signup inserts the profile), but a row inserted out of band leaves
/// the profile missing and `get_or_create` creates it with the model defaults —
/// including the random `background_color`.
///
/// # Errors
///
/// A database failure.
pub async fn profile_or_create(client: &Client, user_id: uuid::Uuid) -> Result<Profile, ViewError> {
    if let Some(profile) = load_profile(client, user_id).await? {
        return Ok(profile);
    }
    let timestamp = now();
    let id = uuid::Uuid::new_v4();
    let statement = plane_auth::queries::INSERT_PROFILE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &id,
                &timestamp,
                &timestamp,
                &user_id,
                &JsonText::new("{}"),
                &true,
                &false,
                &JsonText::new(PROFILE_ONBOARDING_STEP),
                &None::<String>,
                &None::<String>,
                &false,
                &None::<uuid::Uuid>,
                &"INDIA",
                &None::<JsonText>,
                &false,
                &"",
                &"full",
                &false,
                &false,
                &JsonText::new(MOBILE_ONBOARDING_STEP),
                &false,
                &"en",
                &0_i16,
                &JsonText::new("{}"),
                &format!(
                    "#{}",
                    plane_auth::random::get_random_string(6, "0123456789abcdefABCDEF")
                ),
                &false,
                &false,
                &false,
                &JsonText::new(PRODUCT_TOUR),
            ],
        )
        .await?;
    load_profile(client, user_id)
        .await?
        .ok_or_else(|| ViewError::Unknown("the profile insert lost its row".to_string()))
}

/// `plane/db/models/user.py:25-31` `get_default_onboarding`.
const PROFILE_ONBOARDING_STEP: &str = r#"{"profile_complete": false, "workspace_create": false, "workspace_invite": false, "workspace_join": false}"#;
/// `plane/db/models/user.py:34-39` `get_mobile_default_onboarding`.
const MOBILE_ONBOARDING_STEP: &str =
    r#"{"profile_complete": false, "workspace_create": false, "workspace_join": false}"#;
/// `plane/db/models/user.py:42-49` `get_default_product_tour`.
const PRODUCT_TOUR: &str =
    r#"{"work_items": false, "cycles": false, "modules": false, "intake": false, "pages": false}"#;

/// `get_redirection_path(user)` (`redirection_path.py:8-47`).
///
/// Returns the bare value Python returns. Every value is a slug without a
/// leading `/` (`"onboarding"`, `"invitations"`, `"create-workspace"`, a
/// workspace slug), so `validate_next_path` drops it and the user lands on the
/// app root — the "redirect to last workspace" behaviour is dead in Plane. The
/// queries still run, because they can create the profile and they are part of
/// the observed side effects.
///
/// # Errors
///
/// A database failure.
pub async fn redirection_path(client: &Client, user: &User) -> Result<String, ViewError> {
    let profile = profile_or_create(client, user.id).await?;
    if !profile.is_onboarded {
        return Ok("onboarding".to_string());
    }
    if let Some(workspace_id) = profile.last_workspace_id {
        let statement = queries::AUTH_WORKSPACE_BY_ID_FOR_MEMBER
            .prepare(client)
            .await?;
        if let Some(row) = client
            .query_opt(&statement, &[&workspace_id, &user.id])
            .await?
        {
            let slug: String = row.get(0);
            return Ok(slug);
        }
    }
    let statement = queries::AUTH_FIRST_WORKSPACE_FOR_MEMBER
        .prepare(client)
        .await?;
    if let Some(row) = client.query_opt(&statement, &[&user.id]).await? {
        let slug: String = row.get(0);
        return Ok(slug);
    }
    let statement = queries::AUTH_INVITE_COUNT_FOR_EMAIL.prepare(client).await?;
    let row = client.query_one(&statement, &[&user.email]).await?;
    let count: i64 = row.get(0);
    if count != 0 {
        return Ok("invitations".to_string());
    }
    Ok("create-workspace".to_string())
}

/// `plane/authentication/provider/credentials/email.py`'s provider half.
///
/// The password check and the user lookup happen in [`email_authenticate`]
/// before the adapter runs, because `AuthProvider` only carries the data the
/// shared adapter reads.
struct EmailCredentialProvider {
    user_data: Value,
    code: String,
}

impl AuthProvider for EmailCredentialProvider {
    fn provider(&self) -> &'static str {
        "email"
    }

    fn user_data(&self) -> &Value {
        &self.user_data
    }

    fn code(&self) -> Option<&str> {
        Some(&self.code)
    }
}

/// `EmailProvider.authenticate()` (`provider/credentials/email.py`).
///
/// The order is the provider's: the `ENABLE_EMAIL_PASSWORD` gate in the
/// constructor, then `set_user_data()` (the existing-user check on signup, the
/// password check on sign-in), then the shared
/// `Adapter.complete_login_or_signup()`.
///
/// `callback` is `Some(PostUserAuthWorkflow)` for the app variants and `None`
/// for `/spaces/` — the only difference between the two flows beyond the
/// redirect (`space/email.py:93,178`).
///
/// # Errors
///
/// [`AdapterError::Auth`] for every `AuthenticationException` the views
/// redirect with; [`AdapterError::Internal`] for the 500s.
pub async fn email_authenticate(
    ctx: &ViewCtx<'_>,
    client: &Client,
    email: &str,
    password: &str,
    is_signup: bool,
    is_space: bool,
) -> Result<CompletedLogin, AdapterError> {
    if !instance::email_password_login_enabled(client, ctx.settings).await? {
        return Err(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::EmailPasswordAuthenticationDisabled,
        )));
    }
    if is_signup {
        if load_user_by_email(client, email)
            .await
            .map_err(adapter_internal)?
            .is_some()
        {
            return Err(AdapterError::Auth(AuthFlowError::new(
                AuthErrorCode::UserAlreadyExist,
            )));
        }
    } else {
        let Some(user) = load_user_by_email(client, email)
            .await
            .map_err(adapter_internal)?
        else {
            return Err(AdapterError::Auth(AuthFlowError::with_email(
                AuthErrorCode::UserDoesNotExist,
                email,
            )));
        };
        let valid = check_password_async(password.to_string(), user.password.clone())
            .await
            .map_err(AdapterError::from)?;
        if !valid {
            return Err(AdapterError::Auth(AuthFlowError::with_email(
                AuthErrorCode::AuthenticationFailedSignIn,
                email,
            )));
        }
    }

    let user_data = serde_json::json!({
        "email": email,
        "user": {
            "avatar": "",
            "first_name": "",
            "last_name": "",
            "provider_id": "",
            "is_password_autoset": false,
        },
    });
    let provider = EmailCredentialProvider {
        user_data,
        code: password.to_string(),
    };
    let callback: Option<&dyn AuthCallback> = if is_space {
        None
    } else {
        Some(&PostUserAuthWorkflow)
    };
    // Python reads ENABLE_SIGNUP inside `__check_signup`, which runs only when
    // the adapter is about to create a user. Only the sign-up flow can reach
    // that branch, so the read happens there and sign-in passes a value the
    // adapter never reads.
    let enable_signup = if is_signup {
        instance::signup_enabled(client, ctx.settings)
            .await
            .map_err(AdapterError::from)?
    } else {
        true
    };
    let context = AdapterContext {
        client,
        settings: ctx.settings,
        ctx: ctx.ctx,
        publisher: ctx.celery(),
        cache: ctx.redis,
    };
    complete_login_or_signup(&context, &provider, callback, enable_signup).await
}

/// `plane/authentication/utils/login.py:13-27` `user_login`.
///
/// The session and CSRF handles are the middleware's (`ctx.session`,
/// `ctx.csrf`), so the response phase writes the one cookie and the CSRF
/// response phase rotates the one secret. The `RequestContext` is cloned:
/// `login()` assigns `request.user`, and while Django's request logger would
/// see that assignment, Plane's logger re-resolves the user from the session
/// after the view for a plain view, so the clone's field is not observable.
/// Cloning keeps the immutable `ViewCtx` contract and never touches the body.
///
/// # Errors
///
/// [`ViewError::Unknown`] for the login/session failures Python lets escape
/// (a session-key collision, a database outage, `base_host` raising).
pub async fn user_login(
    ctx: &ViewCtx<'_>,
    client: &Client,
    user: &mut User,
    flags: LoginFlags,
) -> Result<(), ViewError> {
    let auth = plane_auth::AuthUser::from_model_row(user);
    let csrf_slot = ctx.csrf()?;
    let mut session = ctx.session().lock().await;
    let mut csrf = csrf_slot.lock().await;
    let mut request = ctx.ctx.clone();
    django_user_login(
        client,
        ctx.settings,
        &mut request,
        &mut session,
        &mut csrf,
        &auth,
        flags,
    )
    .await?;
    // `user_logged_in` -> `update_last_login(user)`: `user.last_login = now()`
    // then `user.save(update_fields=["last_login"])`. Django sets only that
    // column, but `User.save()` (H30) still runs on the *instance*: because the
    // row's `token_updated_at` is non-null, the in-memory `token` and
    // `token_updated_at` move a second time and are **not** persisted. A view
    // that serialises the user after `user_login` (384) therefore renders the
    // second token, while the row keeps the first.
    user.last_login = Some(now());
    let prepared = plane_db::hooks::user::before_save(
        user.email.as_deref(),
        &user.display_name,
        user.is_superuser,
        user.is_staff,
        &user.token,
        user.token_updated_at,
        now(),
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    user.email = Some(prepared.email);
    user.display_name = prepared.display_name;
    user.is_staff = prepared.is_staff;
    user.token = prepared.token;
    user.token_updated_at = prepared.token_updated_at;
    Ok(())
}

/// A `ViewError` (a database failure) as the adapter's 500.
fn adapter_internal(error: ViewError) -> AdapterError {
    AdapterError::Internal(format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn python_truthiness_matches_the_adapter() {
        assert!(!truthy(None));
        assert!(!truthy(Some(&Value::Null)));
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!(0))));
        assert!(truthy(Some(&json!("x"))));
        assert!(truthy(Some(&json!(1))));
    }

    #[test]
    fn str_of_none_is_none() {
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&json!(true)), "True");
        assert_eq!(python_str(&json!(-3)), "-3");
    }

    #[test]
    fn the_profile_defaults_are_the_model_literals() {
        assert_eq!(
            PROFILE_ONBOARDING_STEP,
            r#"{"profile_complete": false, "workspace_create": false, "workspace_invite": false, "workspace_join": false}"#
        );
        assert_eq!(
            MOBILE_ONBOARDING_STEP,
            r#"{"profile_complete": false, "workspace_create": false, "workspace_join": false}"#
        );
        assert_eq!(
            PRODUCT_TOUR,
            r#"{"work_items": false, "cycles": false, "modules": false, "intake": false, "pages": false}"#
        );
    }
}
