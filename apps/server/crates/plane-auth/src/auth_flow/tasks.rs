// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `AUTH_FLOWS.md` §2h — the task call sites the `/auth/` flows enqueue, and
//! the `current_site` they pass.
//!
//! The wire format lives in `plane-jobs` ([`plane_jobs::celery::tasks`]); this
//! module adds one function per **Python call expression** so the argument
//! types and the host variant stay at the call site's shape:
//!
//! | Python | Call sites | Arguments (runtime types) |
//! | --- | --- | --- |
//! | `magic_link.delay(email, key, token)` | `views/app/magic.py:57`, `views/space/magic.py:56` | `(str, str, str)` |
//! | `forgot_password.delay(first_name, email, uidb64, token, current_site)` | `views/app/password_management.py:87`, `views/space/password_management.py:99` | `(str, str, str, str, str)` |
//! | `user_activation_email.delay(current_site, user.id)` | `authentication/adapter/base.py:248` | `(str, UUID)` |
//!
//! `TASKS.md` §"enqueue sites" records the same three shapes (rows 17, 28 and
//! 35 of the task table), and `tools/celery/golden/messages.json` pins their
//! `argsrepr`/`kwargsrepr`; the test at the bottom of this file re-checks those
//! three recorded cases.
//!
//! ## Which `base_host` a call site uses
//!
//! `base_host` exists **twice** in Python, with one difference:
//!
//! * `plane/utils/host.py:17-67` **raises** `ImproperlyConfigured("APP_BASE_URL
//!   or WEB_URL is not set")` when `settings.WEB_URL or settings.APP_BASE_URL`
//!   is empty (`:25-28`). [`current_site`] is this copy, and it is the one
//!   `authentication/adapter/base.py:26` imports — so the
//!   `user_activation_email` call site uses the raising variant (the raise is
//!   swallowed by that call site's `except Exception`, so no mail is sent).
//! * `plane/authentication/utils/host.py:16-63` is the same function **without**
//!   the raise: with neither URL set it returns `None` in the `is_app`/default
//!   branches and raises `TypeError` in the `is_admin`/`is_space` branches,
//!   because `None + "/spaces/"` is not a string concatenation. Every view under
//!   `authentication/views/` imports **this** copy ([`view_base_host`]),
//!   including both `forgot_password` call sites: the app one asks for
//!   `is_app=True` (`views/app/password_management.py:85`) and the space one for
//!   `is_space=True` (`views/space/password_management.py:97`).
//!
//! `crate::login::base_host` is the raising copy and is what
//! `authentication/utils/login.py:10` uses for `device_info.domain`.

use plane_core::config::Settings;
use plane_jobs::celery::Message;
use plane_jobs::celery::tasks;
use uuid::Uuid;

use crate::error::AuthError;
use crate::login::{LoginFlags, base_host};

// ---------------------------------------------------------------------------
// Task call sites
// ---------------------------------------------------------------------------

/// `magic_link.delay(email, key, token)`
/// (`plane/bgtasks/magic_link_code_task.py:23`).
///
/// Both call sites pass plain strings: `email` is the request's normalised
/// address, `key` is `"magic_" + email` (`provider/credentials/magic_code.py:91`)
/// and `token` is `str(secrets.randbelow(900000) + 100000)` (`:87`), so the
/// recorded args are `('ada@example.com', 'magic-key', 'magic-token')`.
///
/// A broker failure propagates: neither view wraps `.delay` in a `try` that
/// catches more than `AuthenticationException`, so the request is a 500.
#[allow(
    dead_code,
    reason = "set B (auth-magic) owns the first non-test caller"
)]
#[must_use]
pub fn magic_link(
    email: impl Into<String>,
    key: impl Into<String>,
    token: impl Into<String>,
) -> Message {
    tasks::magic_link(email.into(), key.into(), token.into())
}

/// `forgot_password.delay(first_name, email, uidb64, token, current_site)`
/// (`plane/bgtasks/forgot_password_task.py:23`).
///
/// Five positional strings at both call sites. `uidb64` is
/// `urlsafe_base64_encode(smart_bytes(user.id))` and `token` is
/// `PasswordResetTokenGenerator().make_token(user)`
/// (`views/app/password_management.py:38-42`). `current_site` comes from
/// [`view_base_host`] — `is_app=True` for the app view and `is_space=True` for
/// the space view — so the two call sites produce different URLs.
///
/// A broker failure propagates (the `.delay` is outside the views'
/// `except AuthenticationException`), a 500.
#[allow(
    dead_code,
    reason = "set A (auth-session) owns the first non-test caller"
)]
#[must_use]
pub fn forgot_password(
    first_name: impl Into<String>,
    email: impl Into<String>,
    uidb64: impl Into<String>,
    token: impl Into<String>,
    current_site: impl Into<String>,
) -> Message {
    tasks::forgot_password(
        first_name.into(),
        email.into(),
        uidb64.into(),
        token.into(),
        current_site.into(),
    )
}

/// `user_activation_email.delay(current_site, user.id)`
/// (`plane/bgtasks/user_activation_email_task.py:23`).
///
/// `user.id` is the model's `uuid.UUID`, which Celery's `argsrepr` renders as
/// `UUID('…')` — the only non-string argument of the three tasks here.
/// `current_site` is [`current_site`] (the raising copy), evaluated before the
/// call; `save_user_data` catches both the raise and a publish failure and only
/// logs (`authentication/adapter/base.py:242-251`).
#[allow(
    dead_code,
    reason = "adapter::save_user_data publishes directly today; the next caller is set A/C"
)]
#[must_use]
pub fn user_activation_email(current_site: impl Into<String>, user_id: Uuid) -> Message {
    tasks::user_activation_email(current_site.into(), user_id)
}

// ---------------------------------------------------------------------------
// current_site
// ---------------------------------------------------------------------------

/// `plane/utils/host.py:17-67` — the **raising** copy of `base_host`, used by
/// `authentication/adapter/base.py` for [`user_activation_email`] and by
/// `authentication/utils/login.py` for `device_info.domain`.
///
/// `settings.WEB_URL or settings.APP_BASE_URL` is the base origin; when both are
/// empty the Python raises `ImproperlyConfigured("APP_BASE_URL or WEB_URL is
/// not set")`, which this returns as [`AuthError::ImproperlyConfigured`].
///
/// It is [`crate::login::base_host`] under a call-site name, so the two cannot
/// drift.
///
/// # Errors
///
/// [`AuthError::ImproperlyConfigured`] with neither URL set.
#[allow(
    dead_code,
    reason = "adapter::save_user_data calls crate::login::base_host directly today"
)]
pub fn current_site(settings: &Settings, flags: LoginFlags) -> Result<String, AuthError> {
    base_host(settings, flags)
}

/// `plane/authentication/utils/host.py:16-63` — the copy every
/// `authentication/views/` module imports.
///
/// It is `base_host` **without** the `ImproperlyConfigured` raise:
///
/// * the `is_admin` and `is_space` branches return `base_origin + path`, so
///   with no base origin Python raises `TypeError: can only concatenate str
///   (not "NoneType") to str` — a 500. That is [`AuthError::ImproperlyConfigured`]
///   here (same status, different log line).
/// * the `is_app` and default branches return the base origin itself, which is
///   `None` when neither URL is set. So the result is `Option<String>`: `None`
///   must survive to the caller because `forgot_password.delay(…, None)` sends a
///   JSON `null` in the task body (and `urljoin(None, url)` is just `url`,
///   `urllib/parse.py:urljoin`'s `if not base: return url`).
///
/// `ADMIN_BASE_PATH`/`SPACE_BASE_PATH` get the same leading/trailing-slash fix
/// as the raising copy. An **empty** path becomes `"/"`, not the default
/// (`host.py:35-38`: `""` is a `str`, so the `isinstance` default never runs).
///
/// # Errors
///
/// [`AuthError::ImproperlyConfigured`] for the `TypeError` above.
#[allow(dead_code, reason = "sets A/B/C own the first view callers")]
pub fn view_base_host(settings: &Settings, flags: LoginFlags) -> Result<Option<String>, AuthError> {
    let base_origin = settings
        .urls
        .web_url
        .as_deref()
        .filter(|url| !url.is_empty())
        .or(settings.urls.app_base_url.as_deref());

    if flags.is_admin {
        let path = crate::login::normalize_base_path(&settings.urls.admin_base_path);
        if let Some(url) = &settings.urls.admin_base_url {
            return Ok(Some(format!("{url}{path}")));
        }
        return match base_origin {
            Some(origin) => Ok(Some(format!("{origin}{path}"))),
            None => Err(AuthError::ImproperlyConfigured(
                "can only concatenate str (not \"NoneType\") to str",
            )),
        };
    }
    if flags.is_space {
        let path = crate::login::normalize_base_path(&settings.urls.space_base_path);
        if let Some(url) = &settings.urls.space_base_url {
            return Ok(Some(format!("{url}{path}")));
        }
        return match base_origin {
            Some(origin) => Ok(Some(format!("{origin}{path}"))),
            None => Err(AuthError::ImproperlyConfigured(
                "can only concatenate str (not \"NoneType\") to str",
            )),
        };
    }
    if flags.is_app {
        // `if settings.APP_BASE_URL: return settings.APP_BASE_URL else: return
        // base_origin` — `None` survives.
        return Ok(settings
            .urls
            .app_base_url
            .clone()
            .or_else(|| base_origin.map(str::to_string)));
    }
    Ok(base_origin.map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_jobs::celery::args::{argsrepr, kwargsrepr};
    use serde_json::Value;

    use crate::queries::ALL_QUERIES;

    const GOLDEN: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/celery/golden/messages.json"
    ));

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

    fn recorded(case: &str) -> Value {
        let document: Value = serde_json::from_str(GOLDEN).expect("golden parses");
        document["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| message["case"] == case)
            .unwrap_or_else(|| panic!("golden has no case {case}"))
            .clone()
    }

    /// The three call-site helpers reproduce the recorded Celery message's
    /// `argsrepr`/`kwargsrepr` and task name. `plane-jobs`' own golden test
    /// compares the whole body; this one pins the call-site wrappers, including
    /// the UUID that only `user_activation_email` has.
    #[test]
    fn the_auth_call_sites_match_the_celery_golden() {
        let magic = magic_link("ada@example.com", "magic-key", "magic-token");
        let forgot = forgot_password(
            "Ada",
            "ada@example.com",
            "dXNlcjox",
            "abc-def",
            "http://localhost:3000",
        );
        let activation = user_activation_email(
            "http://localhost:3000",
            Uuid::parse_str("0d1e2f30-4456-4789-9abc-def012345678").expect("uuid"),
        );

        for (case, message) in [
            ("magic_link", &magic),
            ("forgot_password", &forgot),
            ("user_activation_email", &activation),
        ] {
            let golden = recorded(case);
            assert_eq!(
                message.task(),
                golden["task"].as_str().expect("task"),
                "task name for {case}"
            );
            assert_eq!(
                argsrepr(message.args()),
                golden["headers"]["argsrepr"].as_str().expect("argsrepr"),
                "argsrepr for {case}"
            );
            assert_eq!(
                kwargsrepr(message.kwargs()),
                golden["headers"]["kwargsrepr"]
                    .as_str()
                    .expect("kwargsrepr"),
                "kwargsrepr for {case}"
            );
        }

        // The UUID is an object, not a string: `UUID('…')` in Celery's repr.
        assert!(
            argsrepr(activation.args()).contains("UUID('0d1e2f30-4456-4789-9abc-def012345678')")
        );
    }

    #[test]
    fn the_raising_copy_errors_when_no_url_is_set() {
        let none = settings(None, None);
        assert_eq!(
            current_site(&none, LoginFlags::default()),
            Err(AuthError::ImproperlyConfigured(
                "APP_BASE_URL or WEB_URL is not set"
            ))
        );
        // The space branch concatenates, so it errors too.
        assert_eq!(
            view_base_host(
                &none,
                LoginFlags {
                    is_space: true,
                    ..Default::default()
                }
            ),
            Err(AuthError::ImproperlyConfigured(
                "can only concatenate str (not \"NoneType\") to str"
            ))
        );
        // The app and default branches return `None` instead of raising.
        assert_eq!(
            view_base_host(
                &none,
                LoginFlags {
                    is_app: true,
                    ..Default::default()
                }
            ),
            Ok(None)
        );
        assert_eq!(view_base_host(&none, LoginFlags::default()), Ok(None));
    }

    #[test]
    fn the_view_copy_matches_the_four_branches() {
        let both = settings(Some("https://web.plane.so"), Some("https://app.plane.so"));
        assert_eq!(
            view_base_host(&both, LoginFlags::default()),
            Ok(Some("https://web.plane.so".to_string()))
        );
        assert_eq!(
            view_base_host(
                &both,
                LoginFlags {
                    is_app: true,
                    ..Default::default()
                }
            ),
            Ok(Some("https://app.plane.so".to_string()))
        );
        assert_eq!(
            view_base_host(
                &both,
                LoginFlags {
                    is_admin: true,
                    ..Default::default()
                }
            ),
            Ok(Some("https://web.plane.so/god-mode/".to_string()))
        );
        assert_eq!(
            view_base_host(
                &both,
                LoginFlags {
                    is_space: true,
                    ..Default::default()
                }
            ),
            Ok(Some("https://web.plane.so/spaces/".to_string()))
        );

        // `WEB_URL=""` falls through to `APP_BASE_URL`.
        let app_only = settings(Some(""), Some("https://app.plane.so"));
        assert_eq!(
            view_base_host(&app_only, LoginFlags::default()),
            Ok(Some("https://app.plane.so".to_string()))
        );
    }

    #[test]
    fn the_instance_query_is_catalogued() {
        // The §2i read is registered for `schema_check`.
        assert!(
            ALL_QUERIES
                .iter()
                .any(|query| query.name == "auth_load_first_instance")
        );
    }
}
