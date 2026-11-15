// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SessionMiddleware` (`plane/authentication/middleware/session.py`): the
//! request phase selects the cookie and builds the store (in
//! [`super::auth::AuthContext::new`]); this module is the response phase.
//!
//! `process_response` (`session.py:46-92`) has three branches, and the order in
//! which it reads the flags matters:
//!
//! ```text
//! accessed, modified, empty = snapshot          # before any save/load
//! cookie_name = admin-session-id if "instances" in path else session-id
//! if cookie_name in COOKIES and empty:
//!     delete_cookie(cookie_name)                # no HttpOnly, no Secure
//!     Vary: Cookie
//! else:
//!     if accessed: Vary: Cookie
//!     if (modified or SESSION_SAVE_EVERY_REQUEST) and not empty:
//!         max_age/expires from get_expire_at_browser_close/get_expiry_age
//!         if status < 500:
//!             session.save()                    # UpdateError -> SessionInterrupted (500)
//!             set_cookie(cookie_name, session_key, ...)
//! ```
//!
//! A load triggered by `get_expiry_age()` therefore does **not** retroactively
//! add `Vary: Cookie`; the flag was already read. The admin Max-Age comes from
//! `ADMIN_SESSION_COOKIE_AGE` while the row's `expire_date` still comes from the
//! session data (HTTP_CONTRACT §5.1).

use plane_auth::AuthError;
use plane_auth::SessionCookieAction;
use plane_auth::cookies;
use plane_auth::session::SaveError;
use plane_core::context::RequestContext;

use crate::app::AppState;
use crate::middleware::auth::{self, AuthContext, RequestDb};
use crate::response::Resp;

/// The `SessionInterrupted` message (`session.py:80-84`).
const SESSION_INTERRUPTED: &str = "The request's session was deleted before the request completed. \
     The user may have logged out in a concurrent request, for example.";

/// `SessionMiddleware.process_response`.
pub async fn response(
    state: &AppState,
    ctx: &mut RequestContext,
    auth_context: &mut AuthContext,
    db: &mut RequestDb,
    resp: Resp,
) -> Result<Resp, AuthError> {
    let settings = &state.settings;
    let cookie_name = auth::cookie_name(settings, &ctx.path);
    let cookie_sent = ctx.cookie(cookie_name).is_some();

    // `session.py:53-55`: snapshot before anything can load the session.
    let accessed = auth_context.session.accessed;
    let modified = auth_context.session.modified;
    let empty = auth_context.session.is_empty();

    let mut resp = resp;
    if cookie_sent && empty {
        resp.append_header(
            "Set-Cookie",
            cookies::session_delete_cookie(settings, cookie_name),
        );
        resp.patch_vary("Cookie");
        return Ok(resp);
    }

    if accessed {
        resp.patch_vary("Cookie");
    }
    if !(modified || settings.cookies.session_save_every_request) || empty {
        return Ok(resp);
    }

    let status = resp.status.as_u16();
    let client = db.client(state).await?;
    let action = auth_context
        .session
        .response_action(
            client,
            settings,
            settings.secret_key.value(),
            cookie_sent,
            auth_context.is_admin_path,
            status,
        )
        .await
        .map_err(save_error)?;
    match action {
        SessionCookieAction::Save { max_age, expires } => {
            if let Some(key) = auth_context.session.session_key() {
                let header = cookies::session_cookie(settings, cookie_name, key, max_age, expires);
                resp.append_header("Set-Cookie", header);
            }
        }
        SessionCookieAction::Delete => {
            // `response_action` only returns this for a branch handled above.
            resp.append_header(
                "Set-Cookie",
                cookies::session_delete_cookie(settings, cookie_name),
            );
        }
        SessionCookieAction::None => {}
    }
    Ok(resp)
}

fn save_error(error: SaveError) -> AuthError {
    match error {
        // `UpdateError` → `SessionInterrupted` → Django's 500 page.
        SaveError::Update => AuthError::Database(SESSION_INTERRUPTED.to_string()),
        SaveError::Create => AuthError::Database("session key collision".to_string()),
        SaveError::Database(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Method};
    use plane_auth::RequestUser;
    use plane_core::config::{EnvMap, Role, Settings};

    fn settings() -> Settings {
        let env = EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("WEB_URL", "http://localhost");
        Settings::from_source(&env, Role::Api).expect("settings")
    }

    fn context(path: &str, headers: &[(&str, &str)]) -> RequestContext {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("header"),
                axum::http::HeaderValue::from_str(value).expect("value"),
            );
        }
        RequestContext::new(Method::GET, path, "", map, None)
    }

    fn auth_context(settings: &Settings, ctx: &RequestContext) -> AuthContext {
        let mut auth = AuthContext::new(settings, ctx);
        auth.user = RequestUser::Anonymous;
        auth.resolved = true;
        auth
    }

    /// The `Vary` behaviour the skeleton established: only when accessed.
    #[tokio::test]
    async fn vary_cookie_only_when_accessed() {
        let settings = settings();
        let state = crate::middleware::tests::state_without_database(&settings);
        let mut db = RequestDb::default();

        let mut ctx = context("/", &[]);
        let mut auth = auth_context(&settings, &ctx);
        let untouched = response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await
        .expect("response");
        assert!(untouched.header("vary").is_none());
        assert!(
            !db.is_open(),
            "an untouched session must not open a connection"
        );

        let mut ctx = context("/", &[]);
        let mut auth = auth_context(&settings, &ctx);
        auth.session.accessed = true;
        let accessed_response = response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await
        .expect("response");
        assert_eq!(accessed_response.header("vary"), Some("Cookie"));

        // It appends to an existing Vary rather than replacing it.
        let mut existing = Resp::empty(200, "application/json");
        existing.patch_vary("origin");
        let mut ctx = context("/", &[]);
        let mut auth = auth_context(&settings, &ctx);
        auth.session.accessed = true;
        let appended = response(&state, &mut ctx, &mut auth, &mut db, existing)
            .await
            .expect("response");
        assert_eq!(appended.header("vary"), Some("origin, Cookie"));
    }

    /// A cookie whose value is empty makes the store keyless, so the response
    /// deletes the cookie without a query.
    #[tokio::test]
    async fn empty_cookie_is_deleted_without_a_query() {
        let settings = settings();
        let state = crate::middleware::tests::state_without_database(&settings);
        let mut db = RequestDb::default();
        let mut ctx = context("/", &[("cookie", "session-id=")]);
        let mut auth = auth_context(&settings, &ctx);
        assert!(auth.session.is_empty());
        let deleted = response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await
        .expect("response");
        assert_eq!(
            deleted.header("set-cookie"),
            Some(
                "session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax"
            )
        );
        assert_eq!(deleted.header("vary"), Some("Cookie"));
        assert!(!db.is_open());
    }

    /// A 128-character key that no row matches is *not* empty until it is
    /// loaded, so an untouched, unmodified session changes nothing.
    #[tokio::test]
    async fn keyed_unloaded_session_changes_nothing() {
        let settings = settings();
        let state = crate::middleware::tests::state_without_database(&settings);
        let mut db = RequestDb::default();
        let key = "a".repeat(128);
        let mut ctx = context("/", &[("cookie", &format!("session-id={key}"))]);
        let mut auth = auth_context(&settings, &ctx);
        assert!(!auth.session.is_empty());
        let untouched = response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await
        .expect("response");
        assert!(untouched.header("set-cookie").is_none());
        assert!(untouched.header("vary").is_none());
        assert!(!db.is_open());
    }

    /// The admin cookie is selected by a substring test on the path.
    #[tokio::test]
    async fn admin_paths_use_the_admin_cookie() {
        let settings = settings();
        let state = crate::middleware::tests::state_without_database(&settings);
        let mut db = RequestDb::default();

        let mut ctx = context(
            "/api/workspaces/acme/instances/",
            &[("cookie", "admin-session-id=abcdefgh; session-id=12345678")],
        );
        let mut auth = auth_context(&settings, &ctx);
        assert_eq!(auth.session.session_key(), Some("abcdefgh"));
        assert!(auth.is_admin_path);
        response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await
        .expect("response");

        let mut ctx = context(
            "/api/users/me/",
            &[("cookie", "admin-session-id=abcdefgh; session-id=12345678")],
        );
        let mut auth = auth_context(&settings, &ctx);
        assert_eq!(auth.session.session_key(), Some("12345678"));
        assert!(!auth.is_admin_path);
        response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await
        .expect("response");
    }

    /// A modified session needs the database; with Postgres unreachable the
    /// layer returns the error the pipeline turns into a 500.
    #[tokio::test]
    async fn a_modified_session_needs_the_database() {
        let settings = settings();
        let state = crate::middleware::tests::state_without_database(&settings);
        let mut db = RequestDb::default();
        let key = "b".repeat(128);
        let mut ctx = context(
            "/api/users/me/",
            &[("cookie", &format!("session-id={key}"))],
        );
        let mut auth = auth_context(&settings, &ctx);
        auth.session.modified = true;
        let result = response(
            &state,
            &mut ctx,
            &mut auth,
            &mut db,
            Resp::empty(200, "application/json"),
        )
        .await;
        assert!(result.is_err());
    }
}
