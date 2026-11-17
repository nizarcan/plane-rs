// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `APIView.perform_authentication` and Plane's authenticators.
//!
//! `Request._authenticate` (`rest_framework/request.py:386-403`) walks the
//! authenticators **in list order** and stops at the first non-`None` result:
//!
//! ```python
//! for authenticator in self.authenticators:
//!     try:
//!         user_auth_tuple = authenticator.authenticate(self)
//!     except exceptions.APIException:
//!         self._not_authenticated()
//!         raise
//!     if user_auth_tuple is not None:
//!         self._authenticator = authenticator
//!         self.user, self.auth = user_auth_tuple
//!         return
//! self._not_authenticated()
//! ```
//!
//! `_not_authenticated` replaces `request._request.user` with a plain
//! `AnonymousUser`. That replacement is observable through `Vary: Cookie`
//! (HTTP_CONTRACT §1.1): after it, the request logger no longer reads the
//! Django session. The outcome therefore reports
//! [`AuthOutcome::replaced`] so the pipeline can update its own lazy user.
//!
//! The two Plane classes:
//!
//! * `BaseSessionAuthentication` (`plane/authentication/session.py:56-63`) reads
//!   the middleware's `request.user` and, if it is authenticated, runs Plane's
//!   origin-only CSRF check (`plane_auth::origin`). No token, no cookie.
//! * `APIKeyAuthentication` (`plane/api/middleware/api_authentication.py:17-52`)
//!   reads `X-Api-Key`, looks the token up and writes `last_used`.
//!
//! `rest_framework.authentication.SessionAuthentication` (14 routes) reads the
//! same user but runs **Django's** CSRF check: origin/referer plus the masked
//! token from the `csrftoken` cookie and `X-CSRFToken`. Its failure is a
//! `PermissionDenied` whose detail is Django's reason string.

use http::Method;
use plane_auth::csrf;
use plane_auth::{AuthError, RequestUser, SessionSlot, api_key, get_user, origin};
use plane_core::config::Settings;
use plane_core::error::ApiError;

use crate::db::Conn;
use crate::error::ViewError;
use crate::policy::AuthClass;
use crate::request::ViewRequest;

/// What authentication produced.
#[derive(Debug)]
pub struct AuthOutcome {
    pub user: RequestUser,
    /// `_not_authenticated()` ran: the Django request now exposes
    /// `AnonymousUser`, whatever the middleware had before.
    pub replaced: bool,
    /// The session was read (`request.session.accessed`).
    pub session_accessed: bool,
}

/// The lazy `request.user` plus the authenticators.
pub struct Authenticator<'a, 'db> {
    settings: &'a Settings,
    session: &'a SessionSlot,
    conn: &'a mut Conn<'db>,
    /// A user the pipeline resolved before dispatch (the middleware's
    /// `SimpleLazyObject` already evaluated), when there is one.
    resolved: Option<RequestUser>,
}

impl<'a, 'db> Authenticator<'a, 'db> {
    pub fn new(
        settings: &'a Settings,
        session: &'a SessionSlot,
        conn: &'a mut Conn<'db>,
        preloaded: Option<RequestUser>,
    ) -> Self {
        Self {
            settings,
            session,
            conn,
            resolved: preloaded,
        }
    }

    /// `request._request.user`: `django.contrib.auth.get_user`, evaluated once.
    async fn user(&mut self) -> Result<&RequestUser, ViewError> {
        if self.resolved.is_none() {
            let client = self.conn.client().await?;
            let mut session = self.session.lock().await;
            let user = get_user(client, self.settings, &mut session).await?;
            drop(session);
            self.resolved = Some(user);
        }
        match self.resolved.as_ref() {
            Some(user) => Ok(user),
            // Unreachable: the branch above either set it or returned.
            None => Err(ViewError::Unknown("user resolution failed".to_string())),
        }
    }

    /// `request.session.accessed` at this point, for both the success and the
    /// `_not_authenticated()` path.
    pub async fn session_accessed(&self) -> bool {
        self.session.accessed().await
    }

    /// Whether Django's `SimpleLazyObject` has been evaluated. The pipeline
    /// uses this to decide whether the request logger still has to read the
    /// session (HTTP_CONTRACT §1.1).
    pub fn user_was_evaluated(&self) -> bool {
        self.resolved.is_some()
    }

    /// `Request._authenticate` for this view's authenticator list.
    pub async fn authenticate(
        &mut self,
        classes: &[AuthClass],
        request: &ViewRequest<'_>,
    ) -> Result<AuthOutcome, ViewError> {
        for class in classes {
            let authenticated = match class {
                AuthClass::BaseSession | AuthClass::DrfSession => {
                    self.session_user(*class, request).await?
                }
                AuthClass::ApiKey => self.api_key_user(request).await?,
            };
            if let Some(user) = authenticated {
                return Ok(AuthOutcome {
                    user,
                    replaced: false,
                    session_accessed: self.session_accessed().await,
                });
            }
        }
        // `_not_authenticated()`.
        Ok(AuthOutcome {
            user: RequestUser::Anonymous,
            replaced: true,
            session_accessed: self.session_accessed().await,
        })
    }

    /// `SessionAuthentication.authenticate` for both variants.
    async fn session_user(
        &mut self,
        class: AuthClass,
        request: &ViewRequest<'_>,
    ) -> Result<Option<RequestUser>, ViewError> {
        let user = self.user().await?.clone();
        // `if not user or not user.is_active: return None`. An inactive user's
        // session already resolves to Anonymous in `get_user`
        // (`ModelBackend.get_user` refuses inactive users), which is also what
        // `request.user.is_active` would be false for.
        if !user.is_authenticated() {
            return Ok(None);
        }
        // `enforce_csrf` runs only for an authenticated caller.
        match class {
            AuthClass::BaseSession => {
                let settings = self.settings;
                origin::enforce_csrf(
                    &request.method,
                    request.ctx.header("origin"),
                    request.ctx.header("host").unwrap_or_default(),
                    settings,
                )
                .map_err(|detail| ViewError::Api(ApiError::PermissionDenied(detail.to_string())))?;
            }
            AuthClass::DrfSession => {
                enforce_django_csrf(self.settings, request)?;
            }
            AuthClass::ApiKey => {}
        }
        Ok(Some(user))
    }

    /// `APIKeyAuthentication.authenticate` (`api_authentication.py:17-52`).
    async fn api_key_user(
        &mut self,
        request: &ViewRequest<'_>,
    ) -> Result<Option<RequestUser>, ViewError> {
        let header = request.ctx.header(api_key::API_KEY_HEADER);
        let Some(header) = header else {
            return Ok(None);
        };
        if header.is_empty() {
            return Ok(None);
        }
        let client = self.conn.client().await?;
        match api_key::authenticate(client, Some(header)).await {
            Ok(Some(token)) => Ok(Some(RequestUser::from_user((*token.user).clone()))),
            Ok(None) => Ok(None),
            Err(api_key::ApiKeyError::Invalid) => {
                Err(ViewError::Api(ApiError::AuthenticationFailed {
                    detail: api_key::INVALID_API_TOKEN.to_string(),
                    // `APIKeyAuthentication` has no `authenticate_header`, so DRF
                    // coerces the 401 to 403 (`views.py:458-466`).
                    authenticate_header: None,
                }))
            }
            Err(api_key::ApiKeyError::Database(error)) => Err(ViewError::from(error)),
        }
    }
}

/// `CSRFCheck.process_view` (`django/middleware/csrf.py:414-469`) for DRF's
/// `SessionAuthentication.enforce_csrf`: origin, then referer, then the token.
fn enforce_django_csrf(settings: &Settings, request: &ViewRequest<'_>) -> Result<(), ViewError> {
    let ctx = request.ctx;
    // `CSRFCheck.process_view` accepts safe methods before doing anything
    // (`django/middleware/csrf.py:424-425`).
    if matches!(
        ctx.method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) {
        return Ok(());
    }
    // `request.get_host()`; `CommonMiddleware` validated it at layer 4, so the
    // header is usable verbatim.
    let request_host = ctx.header("host").unwrap_or_default();
    // `rest_framework/authentication.py:126-133`:
    // `raise exceptions.PermissionDenied(detail="CSRF Failed: %s" % exc)`.
    let denied = |reason: String| {
        ViewError::Api(ApiError::PermissionDenied(format!("CSRF Failed: {reason}")))
    };
    if let Some(origin) = ctx.header("origin") {
        if !csrf::origin_verified(settings, origin, request_host, ctx.is_secure) {
            return Err(denied(csrf::REASON_BAD_ORIGIN.replace("%s", origin)));
        }
    } else if ctx.is_secure {
        let server_port = settings.rust.bind.port().to_string();
        if let Err(reason) = csrf::check_referer(settings, ctx, request_host, &server_port) {
            return Err(denied(reason));
        }
    }
    match csrf::check_token(settings, ctx) {
        Ok(()) => Ok(()),
        Err(reason) => Err(denied(reason)),
    }
}

/// The error `plane_auth::get_user` can return, re-exported for callers that
/// want to distinguish a database failure.
pub type UserError = AuthError;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyTable;
    use http::HeaderMap;
    use plane_core::context::RequestContext;

    fn request<'a>(
        ctx: &'a RequestContext,
        route: &'a crate::policy::RoutePolicy,
    ) -> crate::request::ViewRequest<'a> {
        crate::request::ViewRequest::new(ctx, route, Vec::new()).expect("query")
    }

    /// `CSRFCheck.process_view` accepts safe methods before any check, so an
    /// authenticated GET on a `rest_framework.authentication.SessionAuthentication`
    /// route must not demand a CSRF cookie (`/api/timezones/`).
    #[test]
    fn drf_session_csrf_skips_safe_methods() {
        let settings = Settings::from_source(
            &plane_core::config::EnvMap::new().set("REDIS_URL", "redis://127.0.0.1:6379/0"),
            plane_core::config::Role::Api,
        )
        .expect("settings");
        let table = PolicyTable::load().expect("routes.json");
        let route = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.timezone.base.TimezoneEndpoint")
            .expect("timezone route");
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            let ctx = RequestContext::new(
                method.clone(),
                "/api/timezones/",
                "",
                HeaderMap::new(),
                None,
            );
            assert_eq!(
                enforce_django_csrf(&settings, &request(&ctx, route)),
                Ok(()),
                "{method} is a safe method"
            );
        }
        let ctx = RequestContext::new(Method::POST, "/api/timezones/", "", HeaderMap::new(), None);
        assert!(enforce_django_csrf(&settings, &request(&ctx, route)).is_err());
    }
}
