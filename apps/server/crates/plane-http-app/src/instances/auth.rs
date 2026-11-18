// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/api/views/admin.py`'s plain views: routes 263, 267 and 268.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `POST /api/instances/admins/sign-in/` | `InstanceAdminSignInEndpoint` | `admin.py:263-399` |
//! | `POST /api/instances/admins/sign-up/` | `InstanceAdminSignUpEndpoint` | `admin.py:79-261` |
//! | `POST /api/instances/admins/sign-out/` | `InstanceAdminSignOutEndpoint` | `admin.py:403-421` |
//!
//! These are plain `django.views.View`s: Django's CSRF middleware guards them,
//! `@throttle_auth_redirect(is_admin=True)` runs `AuthenticationThrottle` by
//! hand **before** the handler, the body arrives as `request.POST`
//! (`support::post_params`) and every answer is a `302` built with
//! `urljoin(base_host(is_admin=True), "?" + urlencode(error.get_error_dict()))`
//! or `urljoin(base, "general/")`.
//!
//! ## Gates and their order
//!
//! Sign-in: bot → inactive → password → instance-admin membership
//! (`admin.py:309-399`). Sign-up: a global `InstanceAdmin.objects.exists()`
//! pre-check, then `select_for_update` on the instance and the same check
//! again — the GHSA-p548-28jp-wr4p fix (`admin.py:219-249`).
//!
//! ## Bugs ported as-is
//!
//! * `is_telemetry_enabled` arrives as the string `"True"`/`"False"` from the
//!   admin UI and is assigned to a `BooleanField` before `save()`; a lowercase
//!   `"false"` is a `ValidationError` inside a plain view → HTML 500
//!   (`admin.py:252-260`).
//! * Sign-up's final `user.save()` rotates `users.token` because
//!   `token_updated_at` was set one line earlier (`admin.py:252`); sign-in's
//!   final `user.save()` rotates it too (`admin.py:389`).
//! * `if not InstanceAdmin.objects.filter(instance=instance, user=user)` is a
//!   truthiness test with no `role` filter (`admin.py:355`), so an
//!   `InstanceAdmin` row with `role=5` can sign in.
//! * Sign-out is idempotent through a bare `except Exception`
//!   (`admin.py:409-421`): a missing `REMOTE_ADDR`/`User-Agent` makes the save
//!   raise and the redirect is still answered, without `logout()`.

use plane_auth::auth_flow::error::{AuthErrorCode, AuthFlowError};
use plane_auth::auth_flow::password::is_password_strong_async;
use plane_auth::auth_flow::redirect::{urlencode, urljoin};
use plane_auth::password::check_password_with_rehash;
use plane_auth::{LoginFlags, make_password_async};
use plane_db::JsonText;
use plane_db::hooks;
use plane_db::models::license::instance::InstanceAdmin;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::instance::instance_first;
use super::queries;
use crate::auth::support;
use crate::common::cache;
use crate::write;

/// Register the routes this module owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .django_view("plane.license.api.views.admin.InstanceAdminSignInEndpoint")?
        .method(http::Method::POST, sign_in)?;
    registry
        .django_view("plane.license.api.views.admin.InstanceAdminSignUpEndpoint")?
        .method(http::Method::POST, sign_up)?;
    registry
        .django_view("plane.license.api.views.admin.InstanceAdminSignOutEndpoint")?
        .method(http::Method::POST, sign_out)?;
    Ok(())
}

fn admin_flags() -> LoginFlags {
    LoginFlags {
        is_app: false,
        is_space: false,
        is_admin: true,
    }
}

/// `urljoin(base_host(is_admin=True), "?" + urlencode(error.get_error_dict()))`
/// followed by `HttpResponseRedirect`.
fn error_redirect(ctx: &ViewCtx<'_>, error: &AuthFlowError) -> Result<ViewResponse, ViewError> {
    let base = support::base_host(ctx.settings, admin_flags())?;
    let target = format!("?{}", urlencode(&error.redirect_params()));
    let url = urljoin(&base, &target).map_err(|error| ViewError::Unknown(error.to_string()))?;
    support::redirect_url(url)
}

/// `urljoin(base_host(is_admin=True), "general/")`.
fn success_redirect(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let base = support::base_host(ctx.settings, admin_flags())?;
    let url = urljoin(&base, "general/").map_err(|error| ViewError::Unknown(error.to_string()))?;
    support::redirect_url(url)
}

/// `POST /api/instances/admins/sign-in/` (`admin.py:263-399`).
fn sign_in<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `@throttle_auth_redirect(is_admin=True)` is the outermost decorator.
        if let Some(response) = support::throttle_redirect(ctx, None, admin_flags()).await? {
            return Ok(response);
        }
        // `@invalidate_cache(path="/api/instances/", user=False)`.
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = ctx.database().await?;
        let params = support::post_params(ctx)?;
        // `request.POST.get("email", False)`: the payload keeps Python's value,
        // so a missing key reaches the redirect as `False`, not `""`.
        let email_raw = params.get("email");
        let password = params.get("password").unwrap_or("");
        let email_payload = email_raw.map_or(serde_json::Value::Bool(false), |value| {
            serde_json::Value::String(value.to_string())
        });
        let email = email_raw.unwrap_or("");

        let Some(instance) = instance_first(client).await? else {
            let error = AuthFlowError::new(AuthErrorCode::InstanceNotConfigured);
            return error_redirect(ctx, &error);
        };
        if email.is_empty() || password.is_empty() {
            let error = AuthFlowError::new(AuthErrorCode::RequiredAdminEmailPassword)
                .field("email", email_payload);
            return error_redirect(ctx, &error);
        }
        let email = email.trim().to_lowercase();
        if !plane_auth::auth_flow::email::validate_email(&email) {
            let error =
                AuthFlowError::new(AuthErrorCode::InvalidAdminEmail).field("email", email.as_str());
            return error_redirect(ctx, &error);
        }
        let user = load_user_by_email(client, &email).await?;
        let Some(mut user) = user else {
            let error = AuthFlowError::new(AuthErrorCode::AdminUserDoesNotExist)
                .field("email", email.as_str());
            return error_redirect(ctx, &error);
        };
        if user.is_bot {
            let error = AuthFlowError::new(AuthErrorCode::AdminAuthenticationFailed)
                .field("email", email.as_str());
            return error_redirect(ctx, &error);
        }
        if !user.is_active {
            let error = AuthFlowError::new(AuthErrorCode::AdminUserDeactivated);
            return error_redirect(ctx, &error);
        }
        // `user.check_password` runs `django.contrib.auth.hashers.check_password`
        // with `AbstractBaseUser`'s setter, so a hash produced with fewer
        // iterations or a short salt is **re-written** on a successful check
        // (`django/contrib/auth/base_user.py:120-129`): the fixture's 17-char
        // salt is such a hash, and Django persists a new salt before the view's
        // own save.
        let (password_ok, replacement) =
            check_password_with_rehash(password.to_string(), user.password.clone())
                .await
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
        if !password_ok {
            let error = AuthFlowError::new(AuthErrorCode::AdminAuthenticationFailed)
                .field("email", email.as_str());
            return error_redirect(ctx, &error);
        }
        if let Some(replacement) = replacement {
            // `AbstractBaseUser.check_password`'s setter saves the one column
            // immediately (`django/contrib/auth/base_user.py:106-112`), before
            // any later gate can answer an error redirect.
            user.password = replacement;
            let statement = client
                .prepare_cached(plane_auth::queries::UPDATE_USER_PASSWORD.sql)
                .await?;
            client
                .execute(&statement, &[&user.id, &user.password])
                .await?;
        }
        let statement = client
            .prepare_cached(queries::INSTANCE_ADMIN_BY_INSTANCE_USER.sql)
            .await?;
        let row = client
            .query_one(&statement, &[&instance.id, &user.id])
            .await?;
        let is_admin: bool = row.get(0);
        if !is_admin {
            let error = AuthFlowError::new(AuthErrorCode::AdminAuthenticationFailed)
                .field("email", email.as_str());
            return error_redirect(ctx, &error);
        }

        // `admin.py:380-389`: the last-active stamp then a full `save()`.
        let now = write::now();
        user.is_active = true;
        user.last_active = Some(now);
        user.last_login_time = Some(now);
        user.last_login_ip = ctx.ctx.client_ip().ok_or(ViewError::Integrity)?;
        user.last_login_uagent = ctx
            .headers()
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .ok_or(ViewError::Integrity)?
            .to_string();
        user.token_updated_at = Some(now);
        let mut user = crate::users_me::write_email::save_user(client, user).await?;

        support::user_login(ctx, client, &mut user, admin_flags()).await?;
        success_redirect(ctx)
    })
}

/// `POST /api/instances/admins/sign-up/` (`admin.py:79-261`).
fn sign_up<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(response) = support::throttle_redirect(ctx, None, admin_flags()).await? {
            return Ok(response);
        }
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = ctx.database().await?;
        let params = support::post_params(ctx)?;
        // `request.POST.get(key, default)` keeps Python's value: `False` for a
        // missing email/first_name, `""` for the two with a string default and
        // `True` for the telemetry flag. `urlencode` stringifies them to
        // `False`/`True` in the redirect.
        let email_raw = params.get("email");
        let first_raw = params.get("first_name");
        let last_raw = params.get("last_name");
        let company_raw = params.get("company_name");
        let telemetry_raw = params.get("is_telemetry_enabled");
        let payload_value = |raw: Option<&str>, default: serde_json::Value| {
            raw.map_or(default, |value| {
                serde_json::Value::String(value.to_string())
            })
        };
        let email_value = payload_value(email_raw, serde_json::Value::Bool(false));
        let first_value = payload_value(first_raw, serde_json::Value::Bool(false));
        let last_value = payload_value(last_raw, serde_json::Value::String(String::new()));
        let company_value = payload_value(company_raw, serde_json::Value::String(String::new()));
        let telemetry_value = payload_value(telemetry_raw, serde_json::Value::Bool(true));
        let email = email_raw.unwrap_or("");
        let password = params.get("password").unwrap_or("");
        let first_name = first_raw.unwrap_or("");
        let last_name = last_raw.unwrap_or("");
        let company_name = company_raw.unwrap_or("");
        let telemetry = telemetry_raw.unwrap_or("True");

        let Some(instance) = instance_first(client).await? else {
            let error = AuthFlowError::new(AuthErrorCode::InstanceNotConfigured);
            return error_redirect(ctx, &error);
        };
        if instance.is_setup_done || instance_admin_exists(client).await? {
            let error = AuthFlowError::new(AuthErrorCode::AdminAlreadyExist);
            return error_redirect(ctx, &error);
        }
        if email.is_empty() || password.is_empty() || first_name.is_empty() {
            let error = AuthFlowError::new(AuthErrorCode::RequiredAdminEmailPasswordFirstName)
                .field("email", email_value.clone())
                .field("first_name", first_value.clone())
                .field("last_name", last_value.clone())
                .field("company_name", company_value.clone())
                .field("is_telemetry_enabled", telemetry_value.clone());
            return error_redirect(ctx, &error);
        }
        let email = email.trim().to_lowercase();
        if !plane_auth::auth_flow::email::validate_email(&email) {
            let error = AuthFlowError::new(AuthErrorCode::InvalidAdminEmail)
                .field("email", email.as_str())
                .field("first_name", first_value.clone())
                .field("last_name", last_value.clone())
                .field("company_name", company_value.clone())
                .field("is_telemetry_enabled", telemetry_value.clone());
            return error_redirect(ctx, &error);
        }
        let exists = user_exists(client, &email).await?;
        if exists {
            let error = AuthFlowError::new(AuthErrorCode::AdminUserAlreadyExist)
                .field("email", email.as_str())
                .field("first_name", first_value.clone())
                .field("last_name", last_value.clone())
                .field("company_name", company_value.clone())
                .field("is_telemetry_enabled", telemetry_value.clone());
            return error_redirect(ctx, &error);
        }
        let strong = is_password_strong_async(password)
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        if !strong {
            let error = AuthFlowError::new(AuthErrorCode::PasswordTooWeak)
                .field("email", email.as_str())
                .field("first_name", first_value)
                .field("last_name", last_value)
                .field("company_name", company_value)
                .field("is_telemetry_enabled", telemetry_value);
            return error_redirect(ctx, &error);
        }

        let telemetry_bool = django_bool_value(telemetry)?;
        let password_hash = make_password_async(Some(password.to_string()))
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;

        // `with transaction.atomic():` (`admin.py:229-261`). The instance row
        // is locked and the guard re-checked; nothing is written before it.
        client
            .batch_execute("BEGIN")
            .await
            .map_err(ViewError::from)?;
        let locked = {
            let statement = client
                .prepare_cached(queries::INSTANCE_BY_ID_FOR_UPDATE.sql)
                .await?;
            match client.query_opt(&statement, &[&instance.id]).await? {
                Some(row) => Some(plane_db::models::license::instance::Instance::from_row(
                    &row, 0,
                )?),
                None => None,
            }
        };
        let Some(mut locked) = locked else {
            return Err(ViewError::Unknown(
                "Instance.DoesNotExist inside the sign-up lock".to_string(),
            ));
        };
        if locked.is_setup_done || instance_admin_exists_transaction(client).await? {
            let _ = client.batch_execute("ROLLBACK").await;
            let error = AuthFlowError::new(AuthErrorCode::AdminAlreadyExist);
            return error_redirect(ctx, &error);
        }

        let timestamp = write::now();
        let prepared =
            hooks::user::before_save(Some(&email), "", false, false, "", None, timestamp)
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let user = User {
            password: password_hash,
            last_login: None,
            id: Uuid::new_v4(),
            username: Uuid::new_v4().simple().to_string(),
            mobile_number: None,
            email: Some(prepared.email),
            display_name: prepared.display_name,
            first_name: first_name.to_string(),
            last_name: last_name.to_string(),
            avatar: String::new(),
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
            is_email_verified: false,
            is_password_autoset: false,
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
        insert_user(client, &user).await?;
        if let Some(defaults) = hooks::user::after_insert(true, user.is_bot) {
            insert_notification_preference(client, &user, defaults).await?;
        }
        insert_profile(client, &user, company_name).await?;

        // `admin.py:252-259`: the last-active stamp then a full `save()`.
        let user_now = write::now();
        let mut user = user;
        user.is_active = true;
        user.last_active = Some(user_now);
        user.last_login_time = Some(user_now);
        user.last_login_ip = ctx.ctx.client_ip().ok_or(ViewError::Integrity)?;
        user.last_login_uagent = ctx
            .headers()
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .ok_or(ViewError::Integrity)?
            .to_string();
        user.token_updated_at = Some(user_now);
        let mut user = save_user_tx(client, user).await?;

        // `InstanceAdmin.objects.create(user=user, instance=instance)` — the
        // request is anonymous here, so both audit columns are NULL.
        let admin_now = write::now();
        let admin = InstanceAdmin {
            created_at: admin_now,
            updated_at: admin_now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            user_id: Some(user.id),
            instance_id: locked.id,
            role: 20,
            is_verified: false,
        };
        {
            let statement = client
                .prepare_cached(queries::INSTANCE_ADMIN_CREATE.sql)
                .await?;
            client
                .execute(
                    &statement,
                    &[
                        &admin.created_at,
                        &admin.updated_at,
                        &admin.created_by_id,
                        &admin.updated_by_id,
                        &admin.deleted_at,
                        &admin.id,
                        &admin.user_id,
                        &admin.instance_id,
                        &admin.role,
                        &admin.is_verified,
                    ],
                )
                .await?;
        }
        locked.is_setup_done = true;
        locked.instance_name = company_name.to_string();
        locked.is_telemetry_enabled = telemetry_bool;
        locked.updated_at = write::now();
        locked.updated_by_id = None;
        save_instance_in(client, &locked).await?;
        client
            .batch_execute("COMMIT")
            .await
            .map_err(ViewError::from)?;

        support::user_login(ctx, client, &mut user, admin_flags()).await?;
        success_redirect(ctx)
    })
}

/// `POST /api/instances/admins/sign-out/` (`admin.py:403-421`).
///
/// The whole body is one `try/except Exception`; the redirect is answered
/// either way, and a failure leaves the session alone (no `logout()`).
fn sign_out<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(user_id) = current_user_id(ctx).await {
            let _ = record_logout(ctx, user_id).await;
        }
        let base = support::base_host(ctx.settings, admin_flags())?;
        let hosts = plane_auth::auth_flow::redirect::settings_allowed_hosts(ctx.settings)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let url =
            plane_auth::auth_flow::redirect::get_safe_redirect_url(&base, Some(""), &[], &hosts)
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
        support::redirect_url(url)
    })
}

async fn current_user_id(ctx: &ViewCtx<'_>) -> Option<Uuid> {
    match ctx.request_user().await {
        Ok(user) => user.id(),
        Err(_) => None,
    }
}

/// The `try` block: `User.objects.get(pk=request.user.id)`, stamp the logout
/// columns, `save()`, `logout(request)`.
async fn record_logout(ctx: &ViewCtx<'_>, user_id: Uuid) -> Result<(), ViewError> {
    let client = ctx.database().await?;
    let statement = client
        .prepare_cached(crate::auth::queries::AUTH_USER_BY_ID.sql)
        .await?;
    let Some(row) = client.query_opt(&statement, &[&user_id]).await? else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let mut user = User::from_row(&row, 0)?;
    user.last_logout_ip = ctx.ctx.client_ip().ok_or(ViewError::Integrity)?;
    user.last_logout_time = Some(write::now());
    crate::users_me::write_email::save_user(client, user).await?;
    ctx.session().flush(client).await?;
    Ok(())
}

async fn load_user_by_email(
    client: &deadpool_postgres::Client,
    email: &str,
) -> Result<Option<User>, ViewError> {
    let statement = client
        .prepare_cached(crate::auth::queries::AUTH_USER_BY_EMAIL.sql)
        .await?;
    match client.query_opt(&statement, &[&email]).await? {
        Some(row) => Ok(Some(User::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

async fn user_exists(client: &deadpool_postgres::Client, email: &str) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::USER_EXISTS_BY_EMAIL.sql)
        .await?;
    let row = client.query_one(&statement, &[&email]).await?;
    Ok(row.get(0))
}

async fn instance_admin_exists(client: &deadpool_postgres::Client) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::INSTANCE_ADMIN_EXISTS.sql)
        .await?;
    let row = client.query_one(&statement, &[]).await?;
    Ok(row.get(0))
}

async fn instance_admin_exists_transaction(
    client: &deadpool_postgres::Client,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::INSTANCE_ADMIN_EXISTS.sql)
        .await?;
    let row = client.query_one(&statement, &[]).await?;
    Ok(row.get(0))
}

/// `BooleanField.to_python` (`django/db/models/fields/__init__.py`): exactly
/// `"t"/"True"/"1"` and `"f"/"False"/"0"`; a lowercase `"true"` is the
/// `ValidationError` a plain view answers with a 500.
fn django_bool_value(value: &str) -> Result<bool, ViewError> {
    match value {
        "t" | "True" | "1" => Ok(true),
        "f" | "False" | "0" => Ok(false),
        other => Err(ViewError::Unknown(format!(
            "ValidationError: ['“{other}” value must be either True or False.']"
        ))),
    }
}

async fn insert_user(client: &deadpool_postgres::Client, user: &User) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(plane_auth::queries::INSERT_USER.sql)
        .await?;
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

async fn insert_notification_preference(
    client: &deadpool_postgres::Client,
    user: &User,
    defaults: hooks::user::NotificationPreferenceDefaults,
) -> Result<(), ViewError> {
    let timestamp = write::now();
    let statement = client
        .prepare_cached(plane_auth::queries::INSERT_USER_NOTIFICATION_PREFERENCE.sql)
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

/// `Profile.objects.create(user=user, company_name=company_name)`
/// (`admin.py:238`), with the model defaults.
async fn insert_profile(
    client: &deadpool_postgres::Client,
    user: &User,
    company_name: &str,
) -> Result<(), ViewError> {
    const PROFILE_ONBOARDING_STEP: &str = r#"{"profile_complete": false, "workspace_create": false, "workspace_invite": false, "workspace_join": false}"#;
    const MOBILE_ONBOARDING_STEP: &str =
        r#"{"profile_complete": false, "workspace_create": false, "workspace_join": false}"#;
    const PRODUCT_TOUR: &str = r#"{"work_items": false, "cycles": false, "modules": false, "intake": false, "pages": false}"#;
    let timestamp = write::now();
    let statement = client
        .prepare_cached(plane_auth::queries::INSERT_PROFILE.sql)
        .await?;
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
                &company_name,
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
    Ok(())
}

/// `instance.save()` inside the sign-up transaction.
async fn save_instance_in(
    client: &deadpool_postgres::Client,
    instance: &plane_db::models::license::instance::Instance,
) -> Result<(), ViewError> {
    let statement = client.prepare_cached(queries::INSTANCE_SAVE.sql).await?;
    client
        .execute(
            &statement,
            &[
                &instance.created_at,
                &instance.updated_at,
                &instance.created_by_id,
                &instance.updated_by_id,
                &instance.deleted_at,
                &instance.instance_name,
                &instance.whitelist_emails,
                &instance.instance_id,
                &instance.current_version,
                &instance.latest_version,
                &instance.edition,
                &instance.domain,
                &instance.last_checked_at,
                &instance.namespace,
                &instance.is_telemetry_enabled,
                &instance.is_support_required,
                &instance.is_setup_done,
                &instance.is_signup_screen_visited,
                &instance.is_verified,
                &instance.is_test,
                &instance.is_current_version_deprecated,
                &instance.id,
            ],
        )
        .await?;
    Ok(())
}

/// `user.save()` for a full-row write **inside the sign-up transaction**:
/// H30's assignments, then Django's `auto_now` timestamp, then Django's
/// 39-column `UPDATE` (`plane/db/models/base.py:23-45`,
/// `django/db/models/base.py:925-1008`).
///
/// The same statements as [`crate::users_me::write_email::save_user`], which
/// takes the request's `Client`; this copy exists because `tokio-postgres`
/// types `Transaction` and `Client` separately and the Python save is inside
/// `transaction.atomic()` (`admin.py:229-261`).
async fn save_user_tx(client: &deadpool_postgres::Client, user: User) -> Result<User, ViewError> {
    let hook_now = write::now();
    let prepared = hooks::user::before_save(
        user.email.as_deref(),
        &user.display_name,
        user.is_superuser,
        user.is_staff,
        &user.token,
        user.token_updated_at,
        hook_now,
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let updated_at = write::now();
    let mut user = user;
    user.email = Some(prepared.email);
    user.token = prepared.token;
    user.token_updated_at = prepared.token_updated_at;
    user.display_name = prepared.display_name;
    user.is_staff = prepared.is_staff;
    user.updated_at = updated_at;
    let statement = client
        .prepare_cached(crate::users_me::write_queries::USER_UPDATE.sql)
        .await?;
    client
        .execute(
            &statement,
            &[
                &user.password,
                &user.last_login,
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
                &user.id,
            ],
        )
        .await?;
    Ok(user)
}
