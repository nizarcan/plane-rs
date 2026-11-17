// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/adapter/base.py` — the adapter core every provider
//! runs through, plus `process_workspace_project_invitations`
//! (`authentication/utils/workspace_project_join.py`).
//!
//! The Python class mixes three things: pure rules (`sanitize_email`,
//! `__check_signup`, `validate_password`), the login/signup write path
//! (`complete_login_or_signup`, `save_user_data`) and provider-specific work
//! (`check_sync_enabled`, `sync_user_data`, avatar download/upload,
//! `create_update_account`). The first two are here; the third is the
//! [`AuthProvider`] trait, whose defaults are the email/magic-code answers
//! (`False`, no sync, no avatar, no account row). The OAuth adapter implements
//! the same trait in P0b (§2j), so sets A/B/C share this entry point.
//!
//! ## Behaviour that looks wrong and is ported as-is
//!
//! * `User.objects.filter(email=…).first()` is an **exact** match even though
//!   the entry point lower-cased the address (`base.py:73`): a row stored with
//!   an upper-case email (a pre-H30 row, or a direct SQL insert) is not found,
//!   and signup then fails on `users.email`'s unique index.
//! * `self.user_data["user"]` is dereferenced without a default at
//!   `base.py:355-359`; a provider that omits `"user"` is a 500, not a
//!   defaulted signup.
//! * `process_workspace_project_invitations` builds `ProjectMember` rows
//!   without `project_id` (`workspace_project_join.py:55-65`), which is
//!   `NOT NULL`; any accepted project invite is a 500 after the workspace
//!   memberships were already inserted (autocommit, no transaction).
//! * The invite lookup in `__check_signup` is exact
//!   (`base.py:108`), so `ENABLE_SIGNUP="0"` rejects a differently-cased
//!   invite.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, SubsecRound, Utc};
use deadpool_postgres::Client;
use plane_cache::drf::generate_cache_key;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_db::JsonText;
use plane_db::hooks::user as user_hooks;
use plane_db::models::user::User;
use plane_jobs::celery::CeleryPublisher;
use plane_jobs::celery::tasks;
use serde_json::Value;
use uuid::Uuid;

use crate::login::{LoginFlags, base_host};
use crate::password::make_password_async;
use crate::random::get_random_string;

use super::email::validate_email;
use super::error::AuthErrorCode;
use super::error::AuthFlowError;
use super::password::is_password_strong_async;

/// A boxed provider future. The trait is object-safe so one
/// `complete_login_or_signup` serves every provider.
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AdapterError>> + Send + 'a>>;

/// `AuthenticationException` (handled by the view) or an uncaught Python
/// exception / database failure (the family's 500).
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterError {
    /// The caller catches it and redirects/answers with `get_error_dict()`.
    Auth(AuthFlowError),
    /// Python would raise: `KeyError`, `AttributeError`, `IntegrityError`,
    /// `ImproperlyConfigured`, … The view maps it to a 500.
    Internal(String),
}

impl AdapterError {
    #[must_use]
    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<crate::error::AuthError> for AdapterError {
    fn from(error: crate::error::AuthError) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<AuthFlowError> for AdapterError {
    fn from(error: AuthFlowError) -> Self {
        Self::Auth(error)
    }
}

impl From<tokio_postgres::Error> for AdapterError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<plane_db::DbError> for AdapterError {
    fn from(error: plane_db::DbError) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<plane_cache::Error> for AdapterError {
    fn from(error: plane_cache::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

/// The provider-specific half of the adapter.
///
/// The defaults are `EmailProvider`/`MagicCodeProvider`: no IDP sync, no
/// account row, no avatar. `OauthAdapter` (P0b) overrides them.
pub trait AuthProvider: Send + Sync {
    /// `Adapter.provider`: `"email"`, `"magic-code"`, `"google"`, …
    fn provider(&self) -> &'static str;

    /// `self.user_data`: `{"email": …, "user": {"first_name": …}}`.
    fn user_data(&self) -> &Value;

    /// `self.code`: the password or magic code.
    fn code(&self) -> Option<&str>;

    /// `bool(self.token_data)`: `false` for email and magic code, so
    /// `create_update_account` is not called (`base.py:401-403`).
    fn has_token_data(&self) -> bool {
        false
    }

    /// `check_sync_enabled()` (`base.py:122-134`), resolved against the
    /// instance configuration when the provider is constructed.
    fn sync_enabled(&self) -> bool {
        false
    }

    /// `sync_user_data(user=user)` (`base.py:265-305`). Only called when
    /// [`Self::sync_enabled`] is true and the user is not signing up.
    fn sync_user_data<'a>(
        &'a self,
        _context: &'a AdapterContext<'a>,
        _user: &'a mut User,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    /// The signup half of `download_and_upload_avatar` (`base.py:136-224`):
    /// sets `user.avatar` (and `avatar_asset_id`) when the provider has an
    /// avatar URL. A no-op for email/magic code, whose `avatar` is `""`.
    ///
    /// The context is needed because the upload is `S3Storage(request=…)`:
    /// the avatar's object key is built by the storage layer and the request
    /// supplies the browser-visible host (`plane/settings/storage.py:42-56`).
    fn upload_signup_avatar<'a>(
        &'a self,
        _context: &'a AdapterContext<'a>,
        _user: &'a mut User,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    /// `create_update_account(user=user)` (`adapter/oauth.py:70-132`), called
    /// only when [`Self::token_data`] is `Some`.
    fn create_update_account<'a>(
        &'a self,
        _client: &'a Client,
        _user: &'a User,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// What a provider or callback may touch, bundled so the traits stay
/// two-argument.
pub struct AdapterContext<'a> {
    pub client: &'a Client,
    pub settings: &'a Settings,
    pub ctx: &'a RequestContext,
    pub publisher: Option<&'a CeleryPublisher>,
    /// `django.core.cache.cache`; `None` only in a process built without Redis.
    pub cache: Option<&'a plane_cache::Redis>,
}

/// `self.callback(user, is_signup, request)`: `post_user_auth_workflow` for
/// the app variants, absent for `/spaces/`.
pub trait AuthCallback: Send + Sync {
    fn call<'a>(
        &'a self,
        context: &'a AdapterContext<'a>,
        user: &'a User,
        is_signup: bool,
    ) -> ProviderFuture<'a, ()>;
}

/// What `complete_login_or_signup` returns.
#[derive(Debug, Clone)]
pub struct CompletedLogin {
    /// The model instance, as the view passes it to `user_login`.
    pub user: User,
    /// `is_signup`: `True` for a newly created user.
    pub is_signup: bool,
}

/// `timezone.now()` at the precision Postgres stores.
fn now() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

/// Python truthiness for the values `user_data` holds.
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// `str(value)` for the scalar types `user_data` can hold.
pub(crate) fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// `Adapter.sanitize_email` (`base.py:62-86`).
///
/// `email` is the raw `user_data["email"]`, which may be absent, `null`, or a
/// non-string (Python's `str()` accepts them). The two errors differ in their
/// payload: the missing/empty branch keeps the original value (so `null`
/// becomes `{"email": null}`), the invalid branch carries the normalised
/// string.
///
/// # Errors
///
/// [`AuthErrorCode::InvalidEmail`] for both branches.
pub fn sanitize_email(email: Option<&Value>) -> Result<String, AdapterError> {
    if !truthy(email) {
        return Err(AdapterError::Auth(match email {
            Some(value) => {
                AuthFlowError::new(AuthErrorCode::InvalidEmail).field("email", value.clone())
            }
            None => AuthFlowError::with_null_email(AuthErrorCode::InvalidEmail),
        }));
    }
    let email = python_str(email.unwrap_or(&Value::Null))
        .to_lowercase()
        .trim_matches(|character: char| {
            // Python's `str.strip()`: `str.isspace()` includes U+001C..U+001F.
            character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
        })
        .to_string();
    if !validate_email(&email) {
        return Err(AdapterError::Auth(AuthFlowError::with_email(
            AuthErrorCode::InvalidEmail,
            &email,
        )));
    }
    Ok(email)
}

/// `Adapter.__check_signup` (`base.py:99-117`).
///
/// `enable_signup` is `get_configuration_value(...)[0] == "1"`; the caller
/// resolves it (§2i; `ENABLE_SIGNUP="0"` disables). An invited address is
/// allowed even when signup is disabled, and the lookup is exact.
///
/// # Errors
///
/// [`AuthErrorCode::SignupDisabled`], or a database failure.
pub async fn signup_allowed(
    client: &Client,
    email: &str,
    enable_signup: bool,
) -> Result<(), AdapterError> {
    if enable_signup {
        return Ok(());
    }
    let statement = crate::queries::INVITE_EXISTS_FOR_EMAIL
        .prepare(client)
        .await?;
    let row = client.query_one(&statement, &[&email]).await?;
    if row.get::<_, bool>(0) {
        return Ok(());
    }
    Err(AdapterError::Auth(AuthFlowError::with_email(
        AuthErrorCode::SignupDisabled,
        email,
    )))
}

/// `Adapter.validate_password` (`base.py:88-97`).
///
/// zxcvbn runs on `spawn_blocking` (see [`super::password`]): a
/// 128-character random-printable password takes about 0.6 s, which must not
/// block a runtime worker.
///
/// # Errors
///
/// [`AuthErrorCode::PasswordTooWeak`], or the 500 Python raises for a missing
/// password or a panic inside zxcvbn.
pub async fn validate_password(password: Option<&str>, email: &str) -> Result<(), AdapterError> {
    let Some(password) = password else {
        // `is_password_strong(None)` is `TypeError: 'NoneType' object is not
        // subscriptable` in Python (`password.py:13`), a 500.
        return Err(AdapterError::Internal(
            "TypeError: 'NoneType' object is not subscriptable".to_string(),
        ));
    };
    let strong = is_password_strong_async(password)
        .await
        .map_err(|error| AdapterError::Internal(error.to_string()))?;
    if strong {
        Ok(())
    } else {
        Err(AdapterError::Auth(AuthFlowError::with_email(
            AuthErrorCode::PasswordTooWeak,
            email,
        )))
    }
}

/// `Adapter.save_user_data` (`base.py:231-251`), including H30 and `auto_now`.
///
/// The five `timezone.now()` calls are kept separate: Python evaluates them at
/// different points (three assignments, H30's own call, then `Model.save`'s
/// `auto_now`), so they are not one instant.
///
/// # Errors
///
/// A database failure, or H30's `NullEmail` `AttributeError`.
pub async fn save_user_data(
    client: &Client,
    settings: &Settings,
    ctx: &RequestContext,
    publisher: Option<&CeleryPublisher>,
    provider: &str,
    mut user: User,
) -> Result<User, AdapterError> {
    user.last_login_medium = provider.to_string();
    user.last_active = Some(now());
    user.last_login_time = Some(now());
    // `get_client_ip` returns `None` without `X-Forwarded-For`/`REMOTE_ADDR`,
    // and `request.META.get("HTTP_USER_AGENT")` returns `None` without the
    // header. Both columns are `NOT NULL`, so Python's save is an
    // `IntegrityError` — a 500 — rather than an empty string.
    user.last_login_ip = ctx.client_ip().ok_or_else(|| {
        AdapterError::Internal(
            "null value in column \"last_login_ip\" of relation \"users\"".to_string(),
        )
    })?;
    user.last_login_uagent = ctx
        .header("user-agent")
        .ok_or_else(|| {
            AdapterError::Internal(
                "null value in column \"last_login_uagent\" of relation \"users\"".to_string(),
            )
        })?
        .to_string();
    let token_now = now();
    user.token_updated_at = Some(token_now);
    let was_inactive = !user.is_active;
    user.is_active = true;

    let prepared = user_hooks::before_save(
        user.email.as_deref(),
        &user.display_name,
        user.is_superuser,
        user.is_staff,
        &user.token,
        user.token_updated_at,
        now(),
    )
    .map_err(|error| AdapterError::Internal(error.to_string()))?;
    user.email = Some(prepared.email);
    user.display_name = prepared.display_name;
    user.is_staff = prepared.is_staff;
    user.token = prepared.token;
    user.token_updated_at = prepared.token_updated_at;
    user.updated_at = now();

    let statement = crate::queries::UPDATE_USER_LOGIN.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &user.email,
                &user.display_name,
                &user.is_staff,
                &user.token,
                &user.token_updated_at,
                &user.avatar,
                &user.avatar_asset_id,
                &user.last_active,
                &user.last_login_time,
                &user.last_login_medium,
                &user.last_login_ip,
                &user.last_login_uagent,
                &user.is_active,
                &user.updated_at,
                &user.id,
            ],
        )
        .await?;

    if was_inactive {
        // "Save first so activation is persisted before the email side-effect
        // fires" (`base.py:242-249`). The whole block is wrapped in
        // `try/except` and only logs.
        if let Some(publisher) = publisher
            && let Ok(site) = base_host(settings, LoginFlags::default())
        {
            let message = tasks::user_activation_email(site, user.id);
            if let Err(error) = publisher.publish(&message).await {
                tracing::warn!(target: "plane.auth", %error, "user_activation_email failed");
            }
        }
    }
    Ok(user)
}

/// `post_user_auth_workflow` (`authentication/utils/user_auth_workflow.py:8-9`):
/// `process_workspace_project_invitations(user=user)`.
///
/// Implement it as the [`AuthCallback`] the app views pass; the `/spaces/`
/// views pass `None`, which is why they never join an accepted invite.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostUserAuthWorkflow;

/// What the invite join left behind, for tests and the response's side-effect
/// accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InviteOutcome {
    pub workspace_memberships: usize,
    pub project_memberships: usize,
    pub workspace_invites_deleted: u64,
    pub project_invites_deleted: u64,
}

impl AuthCallback for PostUserAuthWorkflow {
    fn call<'a>(
        &'a self,
        context: &'a AdapterContext<'a>,
        user: &'a User,
        _is_signup: bool,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            process_workspace_project_invitations(context.client, context.cache, user, now())
                .await?;
            Ok(())
        })
    }
}

/// `process_workspace_project_invitations` (`workspace_project_join.py:15-74`).
///
/// Two `bulk_create(ignore_conflicts=True)` for workspace memberships, one for
/// project memberships, and `.delete()` on both invite querysets — a **soft**
/// delete (`SoftDeletionQuerySet.delete`, `mixins.py:33-38`).
///
/// The cache invalidation runs once per accepted workspace invite and only
/// when a `Redis` handle is configured; without one Django's cache call raises
/// and the view is a 500, so a caller that passes `None` is a test-only
/// configuration.
///
/// # Errors
///
/// A database failure — including the `NOT NULL` violation the project-member
/// insert always produces (see the module note).
pub async fn process_workspace_project_invitations(
    client: &Client,
    cache: Option<&plane_cache::Redis>,
    user: &User,
    timestamp: DateTime<Utc>,
) -> Result<InviteOutcome, AdapterError> {
    let email = user.email.clone().unwrap_or_default();
    let mut outcome = InviteOutcome::default();

    // `WorkspaceMemberInvite.objects.filter(email=…, accepted=True)`.
    let statement = crate::queries::WORKSPACE_INVITES_FOR_EMAIL
        .prepare(client)
        .await?;
    let rows = client.query(&statement, &[&email]).await?;
    let mut workspace_invites = Vec::with_capacity(rows.len());
    for row in &rows {
        workspace_invites.push((
            row.get::<_, Uuid>(0),
            row.get::<_, Uuid>(1),
            row.get::<_, i16>(2),
            row.get::<_, String>(3),
        ));
    }

    let member_statement = crate::queries::INSERT_WORKSPACE_MEMBER
        .prepare(client)
        .await?;
    for (_invite_id, workspace_id, role, _slug) in &workspace_invites {
        let inserted = client
            .execute(
                &member_statement,
                &[
                    &Uuid::new_v4(),
                    &timestamp,
                    &timestamp,
                    &None::<Uuid>,
                    &workspace_id,
                    &user.id,
                    role,
                    &JsonText::new(WORKSPACE_DEFAULT_PROPS),
                    &JsonText::new(WORKSPACE_DEFAULT_PROPS),
                    &JsonText::new(WORKSPACE_ISSUE_PROPS),
                ],
            )
            .await?;
        outcome.workspace_memberships += inserted as usize;
    }

    for (_invite_id, _workspace_id, _role, slug) in &workspace_invites {
        if let Some(cache) = cache {
            let key = generate_cache_key(&format!("/api/workspaces/{slug}/members/"), None);
            let client_cache = plane_cache::Client::new(cache);
            let keys = client_cache.keys(&format!("*{key}*")).await?;
            client_cache.delete_many(&keys).await?;
        }
    }

    // `ProjectMemberInvite.objects.filter(email=…, accepted=True)`.
    let statement = crate::queries::PROJECT_INVITES_FOR_EMAIL
        .prepare(client)
        .await?;
    let rows = client.query(&statement, &[&email]).await?;
    let mut project_invites = Vec::with_capacity(rows.len());
    for row in &rows {
        project_invites.push((
            row.get::<_, Uuid>(0),
            row.get::<_, Uuid>(1),
            row.get::<_, Uuid>(2),
            row.get::<_, i16>(3),
            row.get::<_, Option<Uuid>>(4),
        ));
    }

    for (_invite_id, workspace_id, _project_id, role, created_by) in &project_invites {
        let role = if matches!(role, 5 | 15) { *role } else { 15 };
        let inserted = client
            .execute(
                &member_statement,
                &[
                    &Uuid::new_v4(),
                    &timestamp,
                    &timestamp,
                    created_by,
                    workspace_id,
                    &user.id,
                    &role,
                    &JsonText::new(WORKSPACE_DEFAULT_PROPS),
                    &JsonText::new(WORKSPACE_DEFAULT_PROPS),
                    &JsonText::new(WORKSPACE_ISSUE_PROPS),
                ],
            )
            .await?;
        outcome.workspace_memberships += inserted as usize;
    }

    let project_member_statement = crate::queries::INSERT_PROJECT_MEMBER
        .prepare(client)
        .await?;
    for (_invite_id, workspace_id, _project_id, role, created_by) in &project_invites {
        let role = if matches!(role, 5 | 15) { *role } else { 15 };
        let inserted = client
            .execute(
                &project_member_statement,
                &[
                    &Uuid::new_v4(),
                    &timestamp,
                    &timestamp,
                    created_by,
                    workspace_id,
                    &user.id,
                    &role,
                    &JsonText::new(PROJECT_DEFAULT_PROPS),
                    &JsonText::new(PROJECT_DEFAULT_PROPS),
                    &JsonText::new(PROJECT_DEFAULT_PREFERENCES),
                ],
            )
            .await?;
        outcome.project_memberships += inserted as usize;
    }

    // `workspace_member_invites.delete()` then `project_member_invites.delete()`.
    let statement = crate::queries::SOFT_DELETE_WORKSPACE_INVITES
        .prepare(client)
        .await?;
    outcome.workspace_invites_deleted = client.execute(&statement, &[&email, &timestamp]).await?;
    let statement = crate::queries::SOFT_DELETE_PROJECT_INVITES
        .prepare(client)
        .await?;
    outcome.project_invites_deleted = client.execute(&statement, &[&email, &timestamp]).await?;

    Ok(outcome)
}

/// `Adapter.complete_login_or_signup` (`base.py:307-406`).
///
/// `enable_signup` is the resolved `ENABLE_SIGNUP == "1"` flag (§2i). The
/// callback is `Some(PostUserAuthWorkflow)` for the app variants and `None` for
/// `/spaces/`.
///
/// # Errors
///
/// [`AdapterError::Auth`] for every `AuthenticationException`; everything else
/// is the 500 Python would produce.
pub async fn complete_login_or_signup(
    context: &AdapterContext<'_>,
    provider: &dyn AuthProvider,
    callback: Option<&dyn AuthCallback>,
    enable_signup: bool,
) -> Result<CompletedLogin, AdapterError> {
    let AdapterContext {
        client,
        settings,
        ctx,
        publisher,
        cache: _,
    } = *context;
    let user_data = provider.user_data();
    // `email = self.user_data.get("email")` then `sanitize_email`.
    let email = sanitize_email(user_data.get("email"))?;

    // `User.objects.filter(email=email).first()`.
    let statement = crate::queries::LOAD_USER_FULL_BY_EMAIL
        .prepare(client)
        .await?;
    let row = client.query_opt(&statement, &[&email]).await?;
    let mut user = match &row {
        Some(row) => Some(User::from_row(row, 0)?),
        None => None,
    };

    if let Some(existing) = &user {
        // GHSA-rmmf-rj2q-3rrg: `last_logout_time` is the discriminator, not
        // `last_login_time` (`base.py:320-337`).
        if !existing.is_active && existing.last_logout_time.is_some() {
            return Err(AdapterError::Auth(AuthFlowError::with_email(
                AuthErrorCode::UserAccountDeactivated,
                &email,
            )));
        }
        if existing.is_bot {
            return Err(AdapterError::Auth(AuthFlowError::with_email(
                AuthErrorCode::BotUserLoginForbidden,
                &email,
            )));
        }
    }

    let is_signup = user.is_none();
    if user.is_none() {
        signup_allowed(client, &email, enable_signup).await?;

        // `self.user_data.get("user").get(...)`: a missing `"user"` is
        // `AttributeError: 'NoneType' object has no attribute 'get'`
        // (`base.py:355-359`).
        let Some(profile_data) = user_data.get("user") else {
            return Err(AdapterError::Internal(
                "AttributeError: 'NoneType' object has no attribute 'get'".to_string(),
            ));
        };
        if !profile_data.is_object() {
            return Err(AdapterError::Internal(
                "AttributeError: 'str' object has no attribute 'get'".to_string(),
            ));
        }

        let is_password_autoset = truthy(profile_data.get("is_password_autoset"));
        let password = if is_password_autoset {
            make_password_async(Some(Uuid::new_v4().simple().to_string()))
                .await
                .map_err(AdapterError::internal)?
        } else {
            validate_password(provider.code(), &email).await?;
            make_password_async(Some(provider.code().unwrap_or_default().to_string()))
                .await
                .map_err(AdapterError::internal)?
        };

        let first_name = profile_data
            .get("first_name")
            .filter(|value| truthy(Some(value)))
            .map(python_str)
            .unwrap_or_default();
        let last_name = profile_data
            .get("last_name")
            .filter(|value| truthy(Some(value)))
            .map(python_str)
            .unwrap_or_default();
        // `avatar` is only assigned to the new user when it is **truthy**
        // (`base.py:377-385`), so a `null`/`""`/missing value leaves the
        // model's `""` default; `str(None)` would be `"None"`.
        let avatar = profile_data
            .get("avatar")
            .filter(|value| truthy(Some(value)))
            .map(python_str)
            .unwrap_or_default();

        let timestamp = now();
        let prepared = user_hooks::before_save(Some(&email), "", false, false, "", None, timestamp)
            .map_err(|error| AdapterError::Internal(error.to_string()))?;
        let new_user = User {
            password,
            last_login: None,
            id: Uuid::new_v4(),
            username: Uuid::new_v4().simple().to_string(),
            mobile_number: None,
            email: Some(prepared.email),
            display_name: prepared.display_name,
            first_name,
            last_name,
            avatar,
            avatar_asset_id: None,
            cover_image: None,
            cover_image_asset_id: None,
            date_joined: timestamp,
            created_at: timestamp,
            updated_at: timestamp,
            last_location: String::new(),
            created_location: String::new(),
            is_superuser: false,
            is_managed: false,
            is_password_expired: false,
            is_active: true,
            is_staff: prepared.is_staff,
            is_email_verified: is_password_autoset,
            is_password_autoset,
            is_password_reset_required: false,
            token: prepared.token,
            last_active: Some(timestamp),
            last_login_time: None,
            last_logout_time: None,
            last_login_ip: String::new(),
            last_logout_ip: String::new(),
            last_login_medium: "email".to_string(),
            last_login_uagent: String::new(),
            token_updated_at: None,
            is_bot: false,
            bot_type: None,
            user_timezone: "UTC".to_string(),
            is_email_valid: false,
            masked_at: None,
        };
        insert_user(client, &new_user).await?;

        // H31: the `post_save` receiver, only for a completed insert.
        if let Some(defaults) = user_hooks::after_insert(true, new_user.is_bot) {
            insert_notification_preference(client, &new_user, defaults).await?;
        }

        let mut new_user = new_user;
        // The signup avatar half (`base.py:389-397`); email/magic pass `""`.
        // Python assigns `user.avatar`/`user.avatar_asset` after this insert
        // and does **not** save; the later `save_user_data` full save is what
        // persists them, which is why `UPDATE_USER_LOGIN` carries both columns.
        provider
            .upload_signup_avatar(context, &mut new_user)
            .await?;

        // `Profile.objects.create(user=user)`.
        insert_profile(client, &new_user).await?;
        user = Some(new_user);
    }

    let mut user = user.ok_or_else(|| {
        AdapterError::Internal("complete_login_or_signup lost its user".to_string())
    })?;

    // `if self.check_sync_enabled() and not is_signup: sync_user_data`.
    if provider.sync_enabled() && !is_signup {
        provider.sync_user_data(context, &mut user).await?;
        // `sync_user_data` ends with `user.save()` — a full save that rotates
        // the token when `token_updated_at` is set and moves `updated_at`,
        // even when every field it assigned already had that value.
        save_synced_user(client, &mut user).await?;
    }

    let user = save_user_data(client, settings, ctx, publisher, provider.provider(), user).await?;

    if let Some(callback) = callback {
        callback.call(context, &user, is_signup).await?;
    }

    if provider.has_token_data() {
        provider.create_update_account(client, &user).await?;
    }

    Ok(CompletedLogin { user, is_signup })
}

/// The `users` insert (`INSERT_USER`), bound in the row's column order.
async fn insert_user(client: &Client, user: &User) -> Result<(), AdapterError> {
    let statement = crate::queries::INSERT_USER.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &user.password,
                &user.last_login,
                &user.id,
                &user.username,
                &user.mobile_number,
                &user.email,
                &user.display_name,
                &user.first_name,
                &user.last_name,
                &user.avatar,
                &user.avatar_asset_id,
                &user.cover_image,
                &user.cover_image_asset_id,
                &user.date_joined,
                &user.created_at,
                &user.updated_at,
                &user.last_location,
                &user.created_location,
                &user.is_superuser,
                &user.is_managed,
                &user.is_password_expired,
                &user.is_active,
                &user.is_staff,
                &user.is_email_verified,
                &user.is_password_autoset,
                &user.is_password_reset_required,
                &user.token,
                &user.last_active,
                &user.last_login_time,
                &user.last_logout_time,
                &user.last_login_ip,
                &user.last_logout_ip,
                &user.last_login_medium,
                &user.last_login_uagent,
                &user.token_updated_at,
                &user.is_bot,
                &user.bot_type,
                &user.user_timezone,
                &user.is_email_valid,
                &user.masked_at,
            ],
        )
        .await?;
    Ok(())
}

/// `Profile.objects.get_or_create(user=user)` (`views/app/magic.py:109`, also
/// the first statement of `authentication/utils/redirection_path.py:11`).
///
/// The profile row, or — when the user has none — the inserted row's defaults.
/// Only the columns those two call sites read are returned, so the row's
/// identity (`id`, timestamps) is not part of the answer.
///
/// Django's `get_or_create` catches the `IntegrityError` a concurrent insert
/// produces and re-runs the `get`; this port propagates the error instead, which
/// is a 500 where Django retries. Unreachable for one user in one request; noted
/// rather than reproduced.
///
/// # Errors
///
/// A database failure, or the `NullEmail` `AttributeError` the insert can raise.
pub async fn profile_for_user(client: &Client, user: &User) -> Result<ProfileState, AdapterError> {
    let statement = crate::queries::LOAD_PROFILE_FOR_USER
        .prepare(client)
        .await?;
    if let Some(row) = client.query_opt(&statement, &[&user.id]).await? {
        return Ok(ProfileState {
            is_onboarded: row.get(0),
            last_workspace_id: row.get(1),
        });
    }
    insert_profile(client, user).await?;
    // The model defaults `insert_profile` writes: `is_onboarded=False` and
    // `last_workspace_id=None` (`plane/db/models/user.py:245-246`).
    Ok(ProfileState {
        is_onboarded: false,
        last_workspace_id: None,
    })
}

/// The columns of `profiles` the `/auth/` flows read
/// (`authentication/utils/redirection_path.py:14-30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileState {
    pub is_onboarded: bool,
    pub last_workspace_id: Option<Uuid>,
}

/// H31's `UserNotificationPreference.objects.create(...)`.
async fn insert_notification_preference(
    client: &Client,
    user: &User,
    defaults: user_hooks::NotificationPreferenceDefaults,
) -> Result<(), AdapterError> {
    let timestamp = now();
    let statement = crate::queries::INSERT_USER_NOTIFICATION_PREFERENCE
        .prepare(client)
        .await?;
    client
        .execute(
            &statement,
            &[
                &Uuid::new_v4(),
                &timestamp,
                &timestamp,
                &user.id,
                &defaults.property_change,
                &defaults.state_change,
                &defaults.comment,
                &defaults.mention,
                &defaults.issue_completed,
            ],
        )
        .await?;
    Ok(())
}

/// `Profile.objects.create(user=user)` with the model defaults
/// (`plane/db/models/user.py:200-268`).
async fn insert_profile(client: &Client, user: &User) -> Result<(), AdapterError> {
    let timestamp = now();
    let statement = crate::queries::INSERT_PROFILE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &Uuid::new_v4(),
                &timestamp,
                &timestamp,
                &user.id,
                &JsonText::new("{}"),
                &true,
                &false,
                &JsonText::new(PROFILE_ONBOARDING_STEP),
                &None::<String>,
                &None::<String>,
                &false,
                &None::<Uuid>,
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
                // `get_random_color`: "#" + six `string.hexdigits` characters.
                &format!("#{}", get_random_string(6, "0123456789abcdefABCDEF")),
                &false,
                &false,
                &false,
                &JsonText::new(PRODUCT_TOUR),
            ],
        )
        .await?;
    Ok(())
}

/// `user.save()` after `sync_user_data`: the avatar columns are the only ones
/// the OAuth sync can change besides the names and display name, and H30 plus
/// `auto_now` still run.
async fn save_synced_user(client: &Client, user: &mut User) -> Result<(), AdapterError> {
    let prepared = user_hooks::before_save(
        user.email.as_deref(),
        &user.display_name,
        user.is_superuser,
        user.is_staff,
        &user.token,
        user.token_updated_at,
        now(),
    )
    .map_err(|error| AdapterError::Internal(error.to_string()))?;
    user.email = Some(prepared.email);
    user.display_name = prepared.display_name;
    user.is_staff = prepared.is_staff;
    user.token = prepared.token;
    user.token_updated_at = prepared.token_updated_at;
    user.updated_at = now();
    let statement = crate::queries::UPDATE_USER_SYNC.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &user.email,
                &user.display_name,
                &user.is_staff,
                &user.token,
                &user.token_updated_at,
                &user.first_name,
                &user.last_name,
                &user.avatar,
                &user.avatar_asset_id,
                &user.updated_at,
                &user.id,
            ],
        )
        .await?;
    Ok(())
}

/// `plane/db/models/workspace.py:22-59` `get_default_props`.
pub const WORKSPACE_DEFAULT_PROPS: &str = r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}, "display_properties": {"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}}"#;
/// `plane/db/models/workspace.py:110-111` `get_issue_props`.
pub const WORKSPACE_ISSUE_PROPS: &str =
    r#"{"subscribed": true, "assigned": true, "created": true, "all_issues": true}"#;
/// `plane/db/models/project.py:39-61` `get_default_props`.
pub const PROJECT_DEFAULT_PROPS: &str = r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}}"#;
/// `plane/db/models/project.py:64-65` `get_default_preferences`.
pub const PROJECT_DEFAULT_PREFERENCES: &str = r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#;
/// `plane/db/models/user.py:25-31` `get_default_onboarding`.
const PROFILE_ONBOARDING_STEP: &str = r#"{"profile_complete": false, "workspace_create": false, "workspace_invite": false, "workspace_join": false}"#;
/// `plane/db/models/user.py:34-39` `get_mobile_default_onboarding`.
const MOBILE_ONBOARDING_STEP: &str =
    r#"{"profile_complete": false, "workspace_create": false, "workspace_join": false}"#;
/// `plane/db/models/user.py:42-49` `get_default_product_tour`.
const PRODUCT_TOUR: &str =
    r#"{"work_items": false, "cycles": false, "modules": false, "intake": false, "pages": false}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn sanitize_email_lowercases_and_strips() {
        assert_eq!(
            sanitize_email(Some(&json!("  MiXeD@Example.COM "))).expect("email"),
            "mixed@example.com"
        );
        // Django's `user_regex` is ASCII-only in the local part; a non-ASCII
        // domain is accepted (`HOSTNAME_LABEL_PATTERN` starts at U+00A1).
        assert_eq!(
            sanitize_email(Some(&json!("cafe@exämple.com"))).expect("email"),
            "cafe@exämple.com"
        );
    }

    #[test]
    fn a_missing_email_keeps_the_null_payload() {
        let error = sanitize_email(None).expect_err("missing");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error: {error:?}");
        };
        assert_eq!(error.code, AuthErrorCode::InvalidEmail);
        assert_eq!(error.payload, vec![("email".to_string(), Value::Null)]);

        let error = sanitize_email(Some(&Value::Null)).expect_err("null");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error: {error:?}");
        };
        assert_eq!(error.payload, vec![("email".to_string(), Value::Null)]);

        let error = sanitize_email(Some(&json!(""))).expect_err("empty");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error: {error:?}");
        };
        assert_eq!(
            error.payload,
            vec![("email".to_string(), Value::String(String::new()))]
        );
    }

    #[test]
    fn an_invalid_email_carries_the_normalised_string() {
        let error = sanitize_email(Some(&json!("  Not An Email "))).expect_err("invalid");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error: {error:?}");
        };
        assert_eq!(error.code, AuthErrorCode::InvalidEmail);
        assert_eq!(
            error.payload,
            vec![("email".to_string(), json!("not an email"))]
        );
    }

    #[tokio::test]
    async fn password_strength_is_the_python_policy() {
        assert!(
            validate_password(Some("Tr0ub4dor&3xY9!qZ"), "a@b.com")
                .await
                .is_ok()
        );
        let error = validate_password(Some("password"), "a@b.com")
            .await
            .expect_err("weak");
        let AdapterError::Auth(error) = error else {
            panic!("not an auth error: {error:?}");
        };
        assert_eq!(error.code, AuthErrorCode::PasswordTooWeak);
        assert_eq!(error.payload, vec![("email".to_string(), json!("a@b.com"))]);
        // A missing code is Python's TypeError, not a weak-password error.
        assert!(matches!(
            validate_password(None, "a@b.com").await,
            Err(AdapterError::Internal(_))
        ));
        // A long password is answered, not a 500: 200 `a`s score 0, and a
        // strong 128-character tail is accepted.
        assert!(matches!(
            validate_password(Some(&"a".repeat(200)), "a@b.com").await,
            Err(AdapterError::Auth(_))
        ));
        assert!(
            validate_password(
                Some(&format!("{}Tr0ub4dor&3xY9!qZ", "a".repeat(200))),
                "a@b.com"
            )
            .await
            .is_ok()
        );
    }
}
