// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `django.contrib.auth.login` / `logout` and Plane's `user_login`.
//!
//! `django.contrib.auth.login` (`django/contrib/auth/__init__.py:190-240`):
//!
//! 1. `session_auth_hash = user.get_session_auth_hash()`;
//! 2. if the session already names a user and it is a different user, or the
//!    stored hash does not match, `session.flush()` (never reuse another user's
//!    session); otherwise `session.cycle_key()`;
//! 3. write `_auth_user_id`, `_auth_user_backend` and `_auth_user_hash`;
//! 4. `rotate_token(request)` — a new CSRF secret, written by the CSRF
//!    middleware's response phase;
//! 5. send `user_logged_in`, whose only receiver is Django's own
//!    `update_last_login` (`django/contrib/auth/apps.py:26-28`), which writes
//!    `users.last_login = timezone.now()` with
//!    `save(update_fields=["last_login"])` — one UPDATE that leaves
//!    `updated_at` untouched. Plane does not connect a receiver of its own
//!    (grep for `user_logged_in` under `plane/` finds no `@receiver`), but
//!    Django's is always connected.
//!
//! Plane's `user_login` (`plane/authentication/utils/login.py:13-27`) calls
//! `login()`, optionally sets the admin expiry, writes `device_info =
//! {"user_agent", "ip_address", "domain"}` and calls `session.save()`
//! explicitly, so a row always exists even when the middleware would not save.

use chrono::{SubsecRound, Utc};
use deadpool_postgres::Client;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use uuid::Uuid;

use crate::csrf::CsrfState;
use crate::error::AuthError;
use crate::json::Json;
use crate::random::constant_time_compare;
use crate::session::{BACKEND_SESSION_KEY, HASH_SESSION_KEY, SESSION_KEY, SessionStore};
use crate::user::{AuthUser, MODEL_BACKEND};

/// Which cookie/redirect base `user_login` records in `device_info`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoginFlags {
    pub is_app: bool,
    pub is_admin: bool,
    pub is_space: bool,
}

/// `plane/utils/host.py:15-57` (`base_host`), the version `login.py` imports
/// (it raises when neither URL is configured, unlike the copy in
/// `plane/authentication/utils/host.py`).
pub fn base_host(settings: &Settings, flags: LoginFlags) -> Result<String, AuthError> {
    let base_origin = settings
        .urls
        .web_url
        .as_deref()
        .filter(|url| !url.is_empty())
        .or(settings.urls.app_base_url.as_deref())
        .ok_or(AuthError::ImproperlyConfigured(
            "APP_BASE_URL or WEB_URL is not set",
        ))?;

    if flags.is_admin {
        let path = normalize_base_path(&settings.urls.admin_base_path);
        return match &settings.urls.admin_base_url {
            Some(url) => Ok(format!("{url}{path}")),
            None => Ok(format!("{base_origin}{path}")),
        };
    }
    if flags.is_space {
        let path = normalize_base_path(&settings.urls.space_base_path);
        return match &settings.urls.space_base_url {
            Some(url) => Ok(format!("{url}{path}")),
            None => Ok(format!("{base_origin}{path}")),
        };
    }
    if flags.is_app {
        return Ok(settings
            .urls
            .app_base_url
            .clone()
            .unwrap_or_else(|| base_origin.to_string()));
    }
    Ok(base_origin.to_string())
}

/// The `isinstance(..., str)` / leading / trailing slash fix-ups in
/// `base_host` (`plane/utils/host.py:32-53`).
///
/// The Python substitutes its default **only** when the setting is not a
/// `str` (`getattr(settings, "ADMIN_BASE_PATH", None)` → `None`); an empty
/// string is a `str`, so it becomes `"/"` (not `"/god-mode/"`). `plane-core`
/// already applies the environment default when the variable is unset, so the
/// only job left is the two slash fix-ups.
pub(crate) fn normalize_base_path(raw: &str) -> String {
    let mut path = raw.to_string();
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    if !path.ends_with('/') {
        path.push('/');
    }
    path
}

/// `django.contrib.auth.login(request, user, backend=None)`.
pub async fn login(
    client: &Client,
    settings: &Settings,
    ctx: &mut RequestContext,
    session: &mut SessionStore,
    csrf: &mut CsrfState,
    user: &AuthUser,
    backend: Option<&str>,
) -> Result<(), AuthError> {
    let secret_key = settings.secret_key.value();
    let session_auth_hash = user.get_session_auth_hash(secret_key);

    if session.contains(client, secret_key, SESSION_KEY).await? {
        let stored_id = session
            .get(client, secret_key, SESSION_KEY)
            .await?
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let stored_id = Uuid::parse_str(&stored_id).map_err(|_| AuthError::InvalidSessionUser)?;
        let stored_hash = session
            .get(client, secret_key, HASH_SESSION_KEY)
            .await?
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let hash_matches = stored_hash.is_empty()
            || constant_time_compare(stored_hash.as_bytes(), session_auth_hash.as_bytes());
        if stored_id != user.id || !hash_matches {
            // Never reuse another user's session.
            session.flush(client).await?;
        }
    } else {
        session
            .cycle_key(client, settings, secret_key)
            .await
            .map_err(save_error)?;
    }

    let backend = backend.unwrap_or(MODEL_BACKEND);
    session
        .set(
            client,
            secret_key,
            SESSION_KEY,
            Json::Str(user.id.to_string()),
        )
        .await?;
    session
        .set(
            client,
            secret_key,
            BACKEND_SESSION_KEY,
            Json::Str(backend.to_string()),
        )
        .await?;
    session
        .set(
            client,
            secret_key,
            HASH_SESSION_KEY,
            Json::Str(session_auth_hash),
        )
        .await?;

    // `request.user = user` when the attribute exists.
    ctx.user = Some(plane_core::ids::UserId(user.id));
    // `rotate_token(request)`.
    csrf.rotate();
    // `user_logged_in.send(...)`: Django's `AuthConfig.ready()` connects
    // `update_last_login`, which writes `last_login` with
    // `save(update_fields=["last_login"])` (`django/contrib/auth/apps.py:26-28`,
    // `models.py:18-22`). `update_fields` leaves `updated_at` alone.
    let statement = client
        .prepare_cached(crate::queries::UPDATE_USER_LAST_LOGIN.sql)
        .await?;
    client
        // Python's `datetime` carries microseconds; Postgres would round finer
        // input, so truncate as `timezone.now()` does.
        .execute(
            &statement,
            &[&plane_core::clock::now().trunc_subsecs(6), &user.id],
        )
        .await?;
    Ok(())
}

/// `django.contrib.auth.logout(request)`: flush the session and set
/// `request.user = AnonymousUser()`.
///
/// The signal has no receiver; Plane's `SignOutAuthEndpoint` additionally
/// writes `last_logout_ip`/`last_logout_time` and saves the user
/// (`plane/authentication/views/app/signout.py:16-28`), which belongs to the
/// sign-out route in Phase 6.
pub async fn logout(
    client: &Client,
    ctx: &mut RequestContext,
    session: &mut SessionStore,
) -> Result<(), AuthError> {
    session.flush(client).await?;
    ctx.user = None;
    Ok(())
}

/// `plane/authentication/utils/login.py:13-27` `user_login`.
pub async fn user_login(
    client: &Client,
    settings: &Settings,
    ctx: &mut RequestContext,
    session: &mut SessionStore,
    csrf: &mut CsrfState,
    user: &AuthUser,
    flags: LoginFlags,
) -> Result<(), AuthError> {
    login(client, settings, ctx, session, csrf, user, None).await?;

    // "If is admin cookie set the custom age".
    if flags.is_admin {
        session
            .set_expiry_seconds(
                client,
                settings.secret_key.value(),
                settings.cookies.admin_session_cookie_age,
            )
            .await?;
    }

    let device_info = Json::Object(vec![
        (
            "user_agent".to_string(),
            Json::Str(ctx.header("user-agent").unwrap_or("").to_string()),
        ),
        (
            "ip_address".to_string(),
            match ctx.client_ip() {
                Some(ip) => Json::Str(ip),
                None => Json::Null,
            },
        ),
        ("domain".to_string(), Json::Str(base_host(settings, flags)?)),
    ]);
    session
        .set(
            client,
            settings.secret_key.value(),
            "device_info",
            device_info,
        )
        .await?;
    // `request.session.save()`, explicitly.
    session
        .save(client, settings, settings.secret_key.value(), false)
        .await
        .map_err(save_error)?;
    Ok(())
}

fn save_error(error: crate::session::SaveError) -> AuthError {
    match error {
        crate::session::SaveError::Database(error) => error,
        other => AuthError::Database(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(web: Option<&str>, app: Option<&str>) -> Settings {
        let mut env =
            plane_core::config::EnvMap::new().set("REDIS_URL", "redis://127.0.0.1:6379/0");
        if let Some(web) = web {
            env = env.set("WEB_URL", web);
        }
        if let Some(app) = app {
            env = env.set("APP_BASE_URL", app);
        }
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    #[test]
    fn base_host_matches_plane() {
        let app = settings(Some("https://web.plane.so"), Some("https://app.plane.so"));
        assert_eq!(
            base_host(&app, LoginFlags::default()).expect("host"),
            "https://web.plane.so"
        );
        assert_eq!(
            base_host(
                &app,
                LoginFlags {
                    is_app: true,
                    ..Default::default()
                }
            )
            .expect("host"),
            "https://app.plane.so"
        );
        assert_eq!(
            base_host(
                &app,
                LoginFlags {
                    is_admin: true,
                    ..Default::default()
                }
            )
            .expect("host"),
            "https://web.plane.so/god-mode/"
        );
        assert_eq!(
            base_host(
                &app,
                LoginFlags {
                    is_space: true,
                    ..Default::default()
                }
            )
            .expect("host"),
            "https://web.plane.so/spaces/"
        );

        // WEB_URL empty falls through to APP_BASE_URL; both unset raises.
        let app_only = settings(Some(""), Some("https://app.plane.so"));
        assert_eq!(
            base_host(&app_only, LoginFlags::default()).expect("host"),
            "https://app.plane.so"
        );
        let none = settings(None, None);
        assert_eq!(
            base_host(&none, LoginFlags::default()),
            Err(AuthError::ImproperlyConfigured(
                "APP_BASE_URL or WEB_URL is not set"
            ))
        );
    }

    #[test]
    fn paths_are_normalized() {
        assert_eq!(normalize_base_path("/god-mode/"), "/god-mode/");
        assert_eq!(normalize_base_path("god-mode"), "/god-mode/");
        // `""` is a `str`, so the Python `isinstance` default never runs and
        // the result is `"/"` (`plane/utils/host.py:35-38`).
        assert_eq!(normalize_base_path(""), "/");
        assert_eq!(normalize_base_path("/x"), "/x/");
    }
}
