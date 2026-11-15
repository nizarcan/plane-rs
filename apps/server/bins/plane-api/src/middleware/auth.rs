// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The session/user state the pipeline threads through a request.
//!
//! Django keeps all of this on the request object: `request.session` is a
//! `SessionStore` built at layer 3 from the cookie, `request.user` is a
//! `SimpleLazyObject` installed at layer 6 and evaluated the first time
//! something reads it (`django/contrib/auth/middleware.py:30-41`). The Rust
//! pipeline makes the same state explicit:
//!
//! * [`RequestDb`] is the per-request database connection, checked out lazily
//!   on the first statement. Django also opens the connection lazily, which is
//!   why `GET /` works with Postgres down and must keep working here.
//! * [`AuthContext`] owns the session, the CSRF cookie state and the resolved
//!   user, plus the `resolved` flag that stands in for Django's lazy object
//!   having been evaluated.
//!
//! The user is only loaded when something asks for it. In the current pipeline
//! that is the request logger at layer 12 (for every route except `GET /`) and
//! the `/_plane_rs/test/` views; Phase 3's DRF runtime will call
//! [`AuthContext::resolve`] from its authentication step.

use deadpool_postgres::Client;
use plane_auth::{AuthError, CsrfState, RequestUser, SessionStore, get_user};
use plane_core::config::Settings;
use plane_core::context::RequestContext;

use crate::app::AppState;

/// `"instances" in request.path` (`plane/authentication/middleware/session.py:23,43`).
///
/// A substring test, not a prefix test: `/api/workspaces/acme/instances/` reads
/// and writes `admin-session-id` (HTTP_CONTRACT §5.1).
pub fn is_admin_path(path: &str) -> bool {
    path.contains("instances")
}

/// The cookie name the middleware selects for this path.
pub fn cookie_name<'a>(settings: &'a Settings, path: &str) -> &'a str {
    if is_admin_path(path) {
        settings.cookies.admin_session_cookie_name
    } else {
        &settings.cookies.session_cookie_name
    }
}

/// A lazily checked-out primary connection, kept for the whole request so the
/// session load, the user load and the session save reuse it (Django's
/// `django.db.connections` does the same).
#[derive(Default)]
pub struct RequestDb {
    client: Option<Client>,
}

impl RequestDb {
    /// The connection, opening it on first use.
    pub async fn client(&mut self, state: &AppState) -> Result<&tokio_postgres::Client, AuthError> {
        if self.client.is_none() {
            let client = state
                .db
                .write_pool()
                .get()
                .await
                .map_err(|error| AuthError::Database(error.to_string()))?;
            self.client = Some(client);
        }
        match &self.client {
            Some(client) => Ok(client),
            // Unreachable: the branch above either set it or returned.
            None => Err(AuthError::Database("no database connection".to_string())),
        }
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.client.is_some()
    }
}

/// `request.session`, the CSRF cookie state and `request.user`.
pub struct AuthContext {
    pub session: SessionStore,
    pub csrf: CsrfState,
    pub user: RequestUser,
    /// The lazy user has been evaluated (or [`AuthContext::resolve`] ran).
    pub resolved: bool,
    /// `"instances" in request.path`, computed once like the middleware does.
    pub is_admin_path: bool,
}

impl AuthContext {
    /// Layer 3's `process_request`: choose the cookie by path and construct the
    /// store. Nothing is read from the database yet.
    pub fn new(settings: &Settings, ctx: &RequestContext) -> Self {
        let is_admin_path = is_admin_path(&ctx.path);
        let name = cookie_name(settings, &ctx.path);
        let key = ctx.cookie(name).map(str::to_string);
        Self {
            session: SessionStore::new(key),
            csrf: CsrfState::default(),
            user: RequestUser::Anonymous,
            resolved: false,
            is_admin_path,
        }
    }

    /// Evaluate `request.user`: `django.contrib.auth.get_user`.
    ///
    /// Idempotent. A request with no session cookie is anonymous without a
    /// database round trip, exactly like Django (no key → no query).
    pub async fn resolve(
        &mut self,
        state: &AppState,
        ctx: &mut RequestContext,
        db: &mut RequestDb,
    ) -> Result<(), AuthError> {
        if self.resolved {
            return Ok(());
        }
        if self.session.session_key().is_none() {
            self.session.load_without_database();
            self.user = RequestUser::Anonymous;
        } else {
            let client = db.client(state).await?;
            self.user = get_user(client, &state.settings, &mut self.session).await?;
        }
        self.resolved = true;
        self.sync(ctx);
        Ok(())
    }

    /// Copy the store's flags onto the request context so the response layers
    /// (and the request log) see what Django's `request.session.accessed` and
    /// `request.user` would show.
    pub fn sync(&self, ctx: &mut RequestContext) {
        if self.session.accessed {
            ctx.session_accessed = true;
        }
        ctx.user = self.user.id().map(plane_core::ids::UserId);
    }

    /// `request.user` after a view assigned one (DRF's `Request.user` setter).
    #[cfg(feature = "test-endpoints")]
    pub fn set_user(&mut self, user: RequestUser, ctx: &mut RequestContext) {
        self.user = user;
        self.resolved = true;
        self.sync(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_core::config::Role;

    fn settings() -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("SESSION_COOKIE_NAME", "session-id");
        Settings::from_source(&env, Role::Api).expect("settings")
    }

    #[test]
    fn admin_cookie_is_a_substring_test() {
        assert!(is_admin_path("/api/instances/"));
        assert!(is_admin_path("/api/workspaces/acme/instances/"));
        assert!(!is_admin_path("/api/users/me/"));
        let settings = settings();
        assert_eq!(
            cookie_name(&settings, "/api/instances/"),
            "admin-session-id"
        );
        assert_eq!(
            cookie_name(&settings, "/api/workspaces/acme/instances/"),
            "admin-session-id"
        );
        assert_eq!(cookie_name(&settings, "/api/users/me/"), "session-id");
    }

    #[test]
    fn no_session_cookie_needs_no_database() {
        let settings = settings();
        let ctx = RequestContext::new(http::Method::GET, "/", "", http::HeaderMap::new(), None);
        let auth = AuthContext::new(&settings, &ctx);
        assert!(auth.session.session_key().is_none());
        assert!(!auth.session.is_empty() || auth.session.session_key().is_none());
        assert!(!auth.resolved);
    }

    #[test]
    fn the_cookie_is_read_from_the_request() {
        let settings = settings();
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            http::HeaderValue::from_static("session-id=12345678"),
        );
        let ctx = RequestContext::new(http::Method::GET, "/api/users/me/", "", headers, None);
        let auth = AuthContext::new(&settings, &ctx);
        assert_eq!(auth.session.session_key(), Some("12345678"));

        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            http::HeaderValue::from_static("admin-session-id=abcdefgh"),
        );
        let ctx = RequestContext::new(http::Method::GET, "/api/instances/", "", headers, None);
        let auth = AuthContext::new(&settings, &ctx);
        assert_eq!(auth.session.session_key(), Some("abcdefgh"));
    }
}
