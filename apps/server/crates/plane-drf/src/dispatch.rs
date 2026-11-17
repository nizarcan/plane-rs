// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `APIView.dispatch` and Plane's view families, in Django's order.
//!
//! | # | Step | Python |
//! | --- | --- | --- |
//! | 1 | `initialize_request` (`view.kwargs`, `format_kwarg`) | `rest_framework/views.py:390-402`, `viewsets.py:146-159` |
//! | 2 | `initial`: format suffix, content negotiation | `views.py:404-417`, `negotiation.py:43-102` |
//! | 3 | `perform_authentication` | `views.py:321-329`, `request.py:386-403` |
//! | 4 | `check_permissions` | `views.py:331-342` |
//! | 5 | `check_throttles` | `views.py:357-376` |
//! | 6 | `TimezoneMixin.initial` | `plane/app/views/base.py:40-45` |
//! | 7 | family `initial` extension | `plane/app/views/base.py:174-198`, `plane/api/views/base.py:59-62`, `plane/space/views/base.py:47-59` |
//! | 8 | `@allow_permission` for this method | `plane/app/permissions/base.py:19-103` |
//! | 9 | the handler, or `options()` | `views.py:517-538` |
//! | 10 | `handle_exception` | `views.py:453-485`, `plane/authentication/adapter/exception.py:17-34` |
//! | 11 | `finalize_response` | `views.py:423-451`, `plane/api/views/base.py:123-136` |
//!
//! `Allow` is on **every** DRF response, including 401/403/404/406/429: it is
//! `default_response_headers`, copied by `finalize_response`, in
//! `http_method_names` order. The value is precomputed by the inventory.
//!
//! An error raised before authentication (406, a bad `?format=`) leaves
//! `request._request.user` unevaluated, so the request logger still evaluates
//! it; `_not_authenticated` replaces it with `AnonymousUser`, so the logger
//! does not (HTTP_CONTRACT §1.1). [`UserOutcome`] carries that distinction.

use std::str::FromStr;

use chrono_tz::Tz;
use http::Method;
use plane_auth::{RequestUser, SessionSlot};
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::{APPLICATION_JSON, ApiError, TEXT_HTML};
use plane_db::DbPools;
use plane_permissions::{Decision, PermCtx, UrlKwargs};
use plane_throttle::{RateLimitHeaders, ThrottleOutcome, ThrottleRequest};

use crate::auth::Authenticator;
use crate::db::Conn;
use crate::error::ViewError;
use crate::metadata::{self, MetadataError};
use crate::negotiate;
use crate::policy::{Family, PolicyTable, RoutePolicy};
use crate::registry::{RegisteredView, Registry};
use crate::request::ViewRequest;
use crate::view::{DrfResponse, ViewCtx, ViewResponse, http_response_not_allowed};

/// What the pipeline has to know after the view ran.
#[derive(Debug)]
pub struct DispatchOutcome {
    pub response: DrfResponse,
    /// DRF's `request.user` (what a handler sees).
    pub user: RequestUser,
    /// What `request._request.user` is after dispatch.
    pub django_user: UserOutcome,
    /// `request.session.accessed`.
    pub session_accessed: bool,
    /// `X-RateLimit-*` values `ApiKeyRateThrottle` put into `request.META`.
    pub rate_limit: Option<RateLimitHeaders>,
}

/// `request._request.user` after dispatch.
#[derive(Debug, Clone)]
pub enum UserOutcome {
    /// Never evaluated: the request logger still evaluates the lazy object.
    Lazy,
    /// `_not_authenticated()` replaced it with `AnonymousUser`.
    Anonymous,
    /// Evaluated and left as it was (`SessionAuthentication.authenticate`).
    Resolved(RequestUser),
}

/// A dispatch request from the pipeline.
pub struct DispatchRequest<'a> {
    /// The resolved resolver entry (`routes.json` `order`).
    pub order: u32,
    /// `view.kwargs`, in URL order.
    pub kwargs: Vec<(&'a str, &'a str)>,
    pub ctx: &'a RequestContext,
    /// `request.session` (owned by the pipeline's `SessionMiddleware` state).
    ///
    /// A shared handle, not `&mut`: the middleware's response phase needs the
    /// same store after the handler ran (Django's `request.session`), and a
    /// handler may `logout()` it.
    pub session: &'a SessionSlot,
    /// `request.META["CSRF_COOKIE"]` (the layer-5 state), shared for the same
    /// reason: `get_token` and `login()` update it and the middleware writes
    /// the one cookie.
    pub csrf: Option<&'a plane_auth::CsrfSlot>,
    /// The middleware's `request.user`, when something already evaluated it.
    pub preloaded_user: Option<RequestUser>,
}

/// The DRF view runtime: the policy, the registered handlers and the process's
/// shared side-effect handles.
pub struct Runtime {
    registry: Registry,
    /// `django.core.cache.cache`: throttles and response caching share one
    /// django-redis client, as they do in Django.
    redis: Option<plane_cache::Redis>,
    celery: Option<plane_jobs::celery::CeleryPublisher>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Runtime")
            .field("registry", &self.registry)
            .field("redis", &self.redis.is_some())
            .field("celery", &self.celery.is_some())
            .finish()
    }
}

/// A startup problem with the runtime wiring.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Registry(#[from] crate::registry::RegistryError),
    #[error(transparent)]
    Policy(#[from] crate::policy::PolicyError),
    #[error(
        "a registered route needs a throttle but no Redis is configured; Django's \
         SimpleRateThrottle would fail the request at runtime"
    )]
    ThrottleWithoutRedis,
}

impl Runtime {
    /// Build the runtime. The registry must already be finished.
    ///
    /// `redis` is the process's one django-redis handle: throttles and the
    /// response cache read and write the same `:1:` keyspace in Django, so they
    /// share it here too. `celery` is the coexistence publisher; both handles
    /// are built at startup and connect lazily, so `/` still answers while
    /// Redis and RabbitMQ are down.
    pub fn new(
        registry: Registry,
        redis: Option<plane_cache::Redis>,
        celery: Option<plane_jobs::celery::CeleryPublisher>,
    ) -> Result<Self, RuntimeError> {
        let needs_throttle = registry.views().keys().any(|view| {
            registry.policy().routes().iter().any(|route| {
                route.view == *view
                    && route.methods.iter().any(|entry| {
                        !route
                            .effective_throttles(entry.action.as_deref())
                            .is_empty()
                    })
            })
        });
        if needs_throttle && redis.is_none() {
            return Err(RuntimeError::ThrottleWithoutRedis);
        }
        Ok(Self {
            registry,
            redis,
            celery,
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The Celery publisher, for the pipeline's token-log layer (which is not a
    /// handler and so has no [`ViewCtx`]).
    pub fn celery(&self) -> Option<&plane_jobs::celery::CeleryPublisher> {
        self.celery.as_ref()
    }

    pub fn policy(&self) -> &PolicyTable {
        self.registry.policy()
    }

    /// Whether the pipeline should route this request here instead of the 501.
    pub fn is_registered(&self, order: u32, method: &Method) -> bool {
        let Some(route) = self.policy().route(order) else {
            return false;
        };
        let Some(registered) = self.registry.view(&route.view) else {
            return false;
        };
        if *method == Method::OPTIONS {
            return registered.is_function() || route.implicit_methods.contains(&Method::OPTIONS);
        }
        if route.methods.iter().any(|entry| entry.method == *method)
            || (*method == Method::HEAD
                && route
                    .methods
                    .iter()
                    .any(|entry| entry.method == Method::GET))
        {
            // Django registers this method: dispatch only a handler that was
            // actually registered, so an unported action keeps the 501.
            return registered.handler(route, method).is_some();
        }
        // The method is not registered for this route at all: Django answers
        // 405, and the runtime must reproduce it. Any registration of the view
        // is enough to own that answer.
        true
    }

    /// Run the request.
    pub async fn dispatch(
        &self,
        settings: &Settings,
        pools: &DbPools,
        request: DispatchRequest<'_>,
    ) -> DispatchOutcome {
        let Some(route) = self.policy().route(request.order) else {
            return internal(ApiError::InternalServerError);
        };
        let Some(registered) = self.registry.view(&route.view) else {
            return internal(ApiError::NotImplemented);
        };

        let method = request.ctx.method.clone();
        let mut view_request = match ViewRequest::new(request.ctx, route, request.kwargs) {
            Ok(view_request) => view_request,
            Err(_) => return family_error(route, ViewError::TooManyFields),
        };
        let mut conn = Conn::new(pools, method, route.use_read_replica);
        let mut state = RunState::default();
        let mut re_raised = false;

        let result = self
            .run(
                settings,
                pools,
                route,
                registered,
                &mut view_request,
                &mut conn,
                request.session,
                request.csrf,
                request.preloaded_user,
                &mut state,
            )
            .await;

        // An exception that escapes `APIView.dispatch` is converted by Django's
        // outer wrapper, so `finalize_response` — and therefore `Allow`, the
        // `X-RateLimit-*` headers and content negotiation — never runs. That is
        // the non-`APIException` path of a family without Plane's
        // `handle_exception` override; a family handler catches it inside
        // `dispatch` and *does* finalize the response it builds.
        let mut response = match result {
            Ok(response) => response,
            Err(error) => {
                // `APIView.dispatch`: when `handle_exception` returns `None` the
                // exception is re-raised, so `finalize_response` never runs and
                // the response Django builds carries **no** `default_response_
                // headers` — no `Allow` (`rest_framework/views.py:453-485`).
                // Only a family with its own `handle_exception` converts a
                // non-`APIException` into a response.
                re_raised = is_re_raised(route.family, &error);
                render_api(route.family.exception(error))
            }
        };

        if route.family.is_drf() && !re_raised {
            if let Some(allow) = &route.allow {
                response.set_header("Allow", allow.clone());
            }
            // Only `BaseAPIView.finalize_response` copies them
            // (`plane/api/views/base.py:123-136`). `BaseViewSet` and the two
            // `drf.APIView` api-roots do not, and their `AnonRateThrottle`
            // never sets `request.META["X-RateLimit-*"]` anyway.
            if copies_rate_limit_headers(route.family)
                && let Some(rate_limit) = state.rate_limit
            {
                for (name, value) in rate_limit.as_headers() {
                    response.set_header(name, value);
                }
            }
        }

        DispatchOutcome {
            response,
            user: state.user.unwrap_or(RequestUser::Anonymous),
            django_user: match state.django_user {
                Some(user) => UserOutcome::Resolved(user),
                None if state.replaced => UserOutcome::Anonymous,
                None => UserOutcome::Lazy,
            },
            session_accessed: state.session_accessed,
            rate_limit: state.rate_limit,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run(
        &self,
        settings: &Settings,
        pools: &DbPools,
        route: &RoutePolicy,
        registered: &RegisteredView,
        request: &mut ViewRequest<'_>,
        conn: &mut Conn<'_>,
        session: &SessionSlot,
        csrf: Option<&plane_auth::CsrfSlot>,
        preloaded_user: Option<RequestUser>,
        state: &mut RunState,
    ) -> Result<DrfResponse, ViewError> {
        let method = request.method.clone();
        // A plain `django.views.View` (or function view) never runs DRF's
        // `initial`: no content negotiation, no authentication classes, no
        // permission classes, no throttles. Django's middleware already
        // handled CSRF, and `View.dispatch` is only method selection plus the
        // handler (`django/views/generic/base.py:134-162`).
        if !route.family.is_drf() {
            let response = Self::run_plain(
                settings,
                pools,
                route,
                registered,
                request,
                method,
                session,
                csrf,
                state,
                self.redis.as_ref(),
                self.celery.as_ref(),
            )
            .await?;
            // A plain Django view can call `logout()` too (`/auth/sign-out/`,
            // Phase 6); the user half belongs to the dispatcher, not the view.
            apply_logout(session, state);
            return Ok(response);
        }

        // ---- initial(): content negotiation -------------------------------
        negotiate::select_renderer(request)?;

        // ---- perform_authentication ---------------------------------------
        let (auth, user_evaluated) = {
            let mut authenticator = Authenticator::new(settings, session, conn, preloaded_user);
            let auth = match authenticator
                .authenticate(&route.authentications, request)
                .await
            {
                Ok(auth) => auth,
                Err(error) => {
                    // `Request._authenticate` catches `APIException`, calls
                    // `_not_authenticated()` and re-raises
                    // (`rest_framework/request.py:389-396`). The replacement is
                    // observable: the request logger no longer reads the
                    // Django session, so `Vary: Cookie` is absent. An error
                    // that is *not* an `APIException` (a database failure)
                    // leaves `request.user` lazy.
                    if matches!(error, ViewError::Api(_)) {
                        state.replaced = true;
                        state.user = Some(RequestUser::Anonymous);
                    }
                    state.session_accessed = authenticator.session_accessed().await;
                    return Err(error);
                }
            };
            let evaluated = authenticator.user_was_evaluated();
            (auth, evaluated)
        };
        state.session_accessed = auth.session_accessed;
        state.user = Some(auth.user.clone());
        if auth.replaced {
            state.replaced = true;
            state.django_user = None;
        } else if user_evaluated {
            state.django_user = Some(auth.user.clone());
        }

        let action = route.action_for(&method);
        let effective_permissions: Vec<plane_permissions::PermissionClass> =
            if route.dynamic_permissions {
                match plane_permissions::dynamic_permission_classes(&route.view, &method, action) {
                    Some(classes) => classes.to_vec(),
                    None => {
                        return Err(ViewError::Unknown(format!(
                            "dynamic permissions for {} have no implementation",
                            route.view
                        )));
                    }
                }
            } else {
                route.permissions.clone()
            };

        // ---- check_permissions --------------------------------------------
        let client = conn.client().await?;
        let kwargs = UrlKwargs::from_iter(request.kwargs.iter().copied());
        let perm_ctx = PermCtx::new(client, &auth.user, &method, &kwargs)
            .with_action(action)
            .with_url_name(route.url_name.as_deref())
            .with_authenticators_configured(!route.authentications.is_empty());
        if let Err(failure) =
            plane_permissions::check_permissions(&effective_permissions, &perm_ctx).await
        {
            return Err(match failure {
                plane_permissions::CheckFailure::Denied(denial) => {
                    ViewError::Api(ApiError::from(denial))
                }
                plane_permissions::CheckFailure::Query(error) => ViewError::from(error),
            });
        }

        // ---- check_throttles ----------------------------------------------
        let classes = route.effective_throttles(action);
        if !classes.is_empty() {
            let remote = request.ctx.remote_addr();
            let throttle_request = ThrottleRequest {
                settings: &settings.throttling,
                user: &auth.user,
                headers: &request.ctx.headers,
                remote_addr: remote.as_deref(),
                asset_id: request.kwarg("asset_id"),
                scope_override: None,
            };
            let redis = self.redis.as_ref().ok_or_else(|| {
                ViewError::Unknown("a throttle ran without a Redis connection".to_string())
            })?;
            match plane_throttle::check_throttles(
                redis,
                classes,
                &throttle_request,
                plane_throttle::now(),
            )
            .await?
            {
                ThrottleOutcome::Allowed { rate_limit } => state.rate_limit = rate_limit,
                ThrottleOutcome::Throttled(rejection) => {
                    return Err(ViewError::Api(rejection.api_error()));
                }
            }
        }

        // ---- TimezoneMixin.initial ----------------------------------------
        let timezone = if route.timezone_mixin && auth.user.is_authenticated() {
            let name = auth
                .user
                .user()
                .map(|user| user.user_timezone.as_str())
                .unwrap_or("UTC");
            match Tz::from_str(name) {
                Ok(tz) => Some(tz),
                Err(_) => {
                    return Err(ViewError::Unknown(format!(
                        "ZoneInfoNotFoundError: No time zone found with key {name}"
                    )));
                }
            }
        } else {
            None
        };

        // ---- family initial extensions ------------------------------------
        match route.family.initial_extra() {
            crate::policy::InitialExtra::BodyMustBeContainer => {
                if matches!(method, Method::POST | Method::PUT | Method::PATCH) {
                    let data = request.data()?;
                    if !data.is_container() {
                        return Err(ViewError::Api(ApiError::ParseError(
                            "Request body must be a JSON object or array.".to_string(),
                        )));
                    }
                }
            }
            crate::policy::InitialExtra::ViewSetGuard => {
                if guard_denies(route, &method, &effective_permissions) {
                    return Err(ViewError::Api(metadata::method_not_allowed(route, &method)));
                }
            }
            crate::policy::InitialExtra::DeployBoardAnchor => {
                if let Some(anchor) = request.kwarg("anchor")
                    && deploy_board_mismatch(client, anchor).await?
                {
                    return Err(ViewError::Api(ApiError::NotFound));
                }
            }
            crate::policy::InitialExtra::None => {}
        }

        // ---- concrete-view `initial()` hook: issue_hidden_from_guest ------
        if route.initial_issue_hidden_from_guest {
            // `plane/app/views/issue/{link,subscriber}.py`: `super().initial()`
            // then `if issue_hidden_from_guest(request, kwargs["slug"],
            // kwargs["project_id"], kwargs["issue_id"]): raise
            // PermissionDenied("You are not allowed to view this issue")`.
            let slug = kwargs.require("slug")?;
            let project_id = kwargs.require("project_id")?;
            let issue_id = kwargs.require("issue_id")?;
            if plane_permissions::issue_hidden_from_guest(&perm_ctx, slug, project_id, issue_id)
                .await?
            {
                return Err(ViewError::Api(ApiError::PermissionDenied(
                    "You are not allowed to view this issue".to_string(),
                )));
            }
        }

        // ---- @allow_permission for this method ----------------------------
        if let Some(entry) = request.method_policy()
            && let Some(rule) = entry.allow
        {
            match plane_permissions::allow_permission(&rule.permission, &perm_ctx).await {
                Ok(Decision::Allow) => {}
                Ok(Decision::Deny(deny)) => {
                    return Ok(DrfResponse::json(
                        403,
                        &serde_json::json!({ "error": deny.error }),
                    )?);
                }
                Err(error) => return Err(ViewError::from(error)),
            }
        }

        // ---- options() ----------------------------------------------------
        if method == Method::OPTIONS && !registered.is_function() {
            if registered.family == Family::DjangoView {
                // `django.views.generic.base.View.options`: an empty
                // `HttpResponse` with `Allow` and `Content-Length: 0`.
                let mut response = DrfResponse::new(200, Some(TEXT_HTML), Vec::new());
                response.set_header("Allow", route.allow.clone().unwrap_or_default());
                response.set_header("Content-Length", "0");
                return Ok(response);
            }
            return self
                .metadata(route, &effective_permissions, client, &auth.user, &kwargs)
                .await;
        }

        // ---- the handler ---------------------------------------------------
        // `View.dispatch` picks `http_method_not_allowed` when the verb is not
        // in `http_method_names`, *after* `initial()` (so the permission
        // classes already ran) and even when the class implements that verb for
        // another URL shape of the same view: route 14 mounts
        // `FileAssetEndpoint.as_view(http_method_names=["get", "delete",
        // "head", "options"])`, while route 13 registers the same class's POST
        // (`plane/app/urls/asset.py:27-36`). Viewsets resolve through
        // `action_for`, which already answers `None` for a verb outside the
        // route's method map.
        if !registered.family.is_viewset() && !route.is_allowed_method(&method) {
            if registered.family == Family::DjangoView {
                return Ok(http_response_not_allowed(
                    route.allow.as_deref().unwrap_or_default(),
                ));
            }
            return Err(ViewError::Api(metadata::method_not_allowed(route, &method)));
        }
        let Some(handler) = registered.handler(route, &method) else {
            if registered.family == Family::DjangoView {
                return Ok(http_response_not_allowed(
                    route.allow.as_deref().unwrap_or_default(),
                ));
            }
            return Err(ViewError::Api(metadata::method_not_allowed(route, &method)));
        };

        let db = Some(client);
        let ctx = ViewCtx {
            request,
            user: &auth.user,
            session,
            csrf,
            action,
            view: &route.view,
            timezone: timezone.as_ref(),
            db,
            pools,
            settings,
            ctx: request.ctx,
            plain: None,
            redis: self.redis.as_ref(),
            publisher: self.celery.as_ref(),
        };
        let response = handler(&ctx).await?;
        apply_logout(session, state);
        Ok(DrfResponse::render(response)?)
    }

    /// `django.views.generic.base.View.dispatch` (`base.py:134-162`) and a
    /// function view. No DRF policy runs.
    #[allow(clippy::too_many_arguments)]
    async fn run_plain(
        settings: &Settings,
        pools: &DbPools,
        route: &RoutePolicy,
        registered: &RegisteredView,
        request: &mut ViewRequest<'_>,
        method: Method,
        session: &SessionSlot,
        csrf: Option<&plane_auth::CsrfSlot>,
        state: &mut RunState,
        redis: Option<&plane_cache::Redis>,
        publisher: Option<&plane_jobs::celery::CeleryPublisher>,
    ) -> Result<DrfResponse, ViewError> {
        // `View.options` answers when the class does not define `options`.
        if method == Method::OPTIONS
            && route.family == Family::DjangoView
            && !registered.has_method(&Method::OPTIONS)
        {
            let mut response = DrfResponse::new(200, Some(TEXT_HTML), Vec::new());
            response.set_header("Allow", route.allow.clone().unwrap_or_default());
            response.set_header("Content-Length", "0");
            return Ok(response);
        }
        let Some(handler) = registered.handler(route, &method) else {
            // `View.http_method_not_allowed` -> `HttpResponseNotAllowed`, and
            // `require_http_methods`' 405 for a function view.
            return Ok(http_response_not_allowed(
                route.allow.as_deref().unwrap_or_default(),
            ));
        };
        // A plain view's `request.user` is Django's lazy object, not DRF's
        // authenticators: it is evaluated on first access (one hash-verified
        // SELECT for a signed-in session) and the connection is opened on the
        // first query. `ctx.user` stays `Anonymous` so a handler that never
        // reads `request.user` behaves exactly as before this change.
        let anonymous = RequestUser::Anonymous;
        let plain = crate::view::PlainViewState::new();
        let ctx = ViewCtx {
            request,
            user: &anonymous,
            session,
            csrf,
            action: None,
            view: &route.view,
            timezone: None,
            db: None,
            pools,
            settings,
            ctx: request.ctx,
            plain: Some(&plain),
            redis,
            publisher,
        };
        let response = DrfResponse::render(handler(&ctx).await?)?;
        if let Some(user) = plain.evaluated_user() {
            let user = user.clone();
            state.django_user = Some(user);
        }
        Ok(response)
    }

    /// `APIView.options` -> `SimpleMetadata().determine_metadata`.
    async fn metadata(
        &self,
        route: &RoutePolicy,
        permissions: &[plane_permissions::PermissionClass],
        client: &deadpool_postgres::Client,
        user: &RequestUser,
        kwargs: &UrlKwargs<'_>,
    ) -> Result<DrfResponse, ViewError> {
        let mut allowed = Vec::new();
        for attempt in metadata::attempt_methods(route) {
            // `check_permissions` calls `get_permissions()`, and the cloned
            // request keeps `view.action == "metadata"` while carrying the
            // attempt's method, so a dynamic override is resolved for that
            // method (`metadata.py:79-87`).
            let dynamic = if route.dynamic_permissions {
                plane_permissions::dynamic_permission_classes(
                    &route.view,
                    &attempt,
                    Some("metadata"),
                )
            } else {
                None
            };
            let classes = dynamic.unwrap_or(permissions);
            let ctx = PermCtx::new(client, user, &attempt, kwargs)
                .with_authenticators_configured(!route.authentications.is_empty());
            if plane_permissions::check_permissions(classes, &ctx)
                .await
                .is_ok()
            {
                allowed.push(attempt);
            }
        }
        match metadata::determine(route, &allowed) {
            Ok(metadata) => Ok(DrfResponse::new(
                200,
                Some(APPLICATION_JSON),
                metadata.render()?,
            )),
            Err(MetadataError::ActionsFailed(error)) => Err(ViewError::Unknown(error)),
            Err(error) => Err(ViewError::from(error)),
        }
    }
}

/// Whether a handler error escapes `APIView.dispatch` altogether.
///
/// `handle_exception` only converts `APIException` subclasses; everything else
/// is re-raised unless the family base implements its own handler
/// (`plane-drf`'s `Family::has_family_exception_handler`). A re-raised
/// exception is answered by Django's handler, so `finalize_response` — and the
/// `Allow` and `X-RateLimit-*` headers it copies — never runs.
fn is_re_raised(family: Family, error: &ViewError) -> bool {
    !matches!(error, ViewError::Api(_)) && !family.has_family_exception_handler()
}

/// Whether `finalize_response` copies `X-RateLimit-*` from `request.META`.
///
/// Only `plane.api.views.base.BaseAPIView` does (`:123-136`).
/// `BaseViewSet` (`:162-279`) inherits DRF's passthrough, and the two v1
/// `drf.APIView` api-roots inherit it from `APIView`. Both of those are
/// throttled by `AnonRateThrottle`, which never writes the two keys.
fn copies_rate_limit_headers(family: Family) -> bool {
    family == Family::ApiApiView
}

/// The action half of `BaseViewSet._resolved_action_is_authorized`
/// (`plane/app/views/base.py:103-172`).
fn guard_denies(
    route: &RoutePolicy,
    method: &Method,
    permissions: &[plane_permissions::PermissionClass],
) -> bool {
    if route.family != Family::AppViewSet {
        return false;
    }
    let Some(action) = route.action_for(method) else {
        return false;
    };
    let Some(guard) = route.guard_actions.get(action) else {
        return false;
    };
    if !guard.mixin_provided {
        return false;
    }
    // `_NON_AUTHORIZING_PERMISSIONS = {IsAuthenticated, AllowAny}`.
    let authorizes = permissions.iter().any(|class| {
        !matches!(
            class,
            plane_permissions::PermissionClass::IsAuthenticated
                | plane_permissions::PermissionClass::AllowAny
        )
    });
    if authorizes {
        return false;
    }
    if action == "create" && guard.project_implements_create {
        return false;
    }
    if action == "partial_update" && guard.project_implements_update {
        return false;
    }
    !guard.project_implemented
}

/// `DeployBoardAnchorMixin.initial` (`plane/space/views/base.py:47-59`).
///
/// `deploy_boards` is soft-deleted (`deleted_at IS NULL`), and Django's
/// `exclude(entity_identifier=F("project_id"))` keeps SQL's three-valued logic:
/// a NULL `entity_identifier` does not match.
async fn deploy_board_mismatch(
    client: &deadpool_postgres::Client,
    anchor: &str,
) -> Result<bool, ViewError> {
    const SQL: &str = "SELECT EXISTS (SELECT 1 FROM deploy_boards \
                       WHERE anchor = $1 AND entity_name = 'project' AND deleted_at IS NULL \
                       AND NOT (entity_identifier = project_id))";
    let statement = client.prepare_cached(SQL).await.map_err(ViewError::from)?;
    let row = client
        .query_one(&statement, &[&anchor])
        .await
        .map_err(ViewError::from)?;
    Ok(row.get(0))
}

#[derive(Default)]
struct RunState {
    user: Option<RequestUser>,
    django_user: Option<RequestUser>,
    replaced: bool,
    session_accessed: bool,
    rate_limit: Option<RateLimitHeaders>,
}

/// `django.contrib.auth.logout`'s user half
/// (`django/contrib/auth/__init__.py:243-258`): after `session.flush()`,
/// `request.user = AnonymousUser`. A handler reaches the store through
/// [`crate::view::ViewCtx::session`], so the dispatcher applies the half it
/// owns when the handler is done — before `finalize_response` and the request
/// logger, exactly when Django's `logout()` runs.
fn apply_logout(session: &SessionSlot, state: &mut RunState) {
    if session.logged_out() {
        state.replaced = true;
        state.user = Some(RequestUser::Anonymous);
        state.django_user = Some(RequestUser::Anonymous);
    }
}

/// Render an [`ApiError`] the way DRF plus Plane's handler do.
pub fn render_api(error: ApiError) -> DrfResponse {
    match error.render() {
        Ok(response) => {
            let mut rendered =
                DrfResponse::new(response.status, Some(response.content_type), response.body);
            for (name, value) in response.headers {
                rendered.set_header(name, value);
            }
            rendered
        }
        Err(error) => {
            tracing::error!(target: "plane.api", %error, "failed to render an error response");
            DrfResponse::new(
                500,
                Some(TEXT_HTML),
                plane_core::error::ERROR_PAGE_500.as_bytes().to_vec(),
            )
        }
    }
}

fn family_error(route: &RoutePolicy, error: ViewError) -> DispatchOutcome {
    DispatchOutcome {
        response: render_api(route.family.exception(error)),
        user: RequestUser::Anonymous,
        django_user: UserOutcome::Lazy,
        session_accessed: false,
        rate_limit: None,
    }
}

fn internal(error: ApiError) -> DispatchOutcome {
    DispatchOutcome {
        response: render_api(error),
        user: RequestUser::Anonymous,
        django_user: UserOutcome::Lazy,
        session_accessed: false,
        rate_limit: None,
    }
}

/// A `ViewResponse::Raw` helper for tests.
pub fn raw_response(status: u16, content_type: &'static str, body: &[u8]) -> ViewResponse {
    ViewResponse::raw(status, content_type, body.to_vec())
}

/// The request context a handler received is the pipeline's; this re-export
/// keeps `use plane_drf::dispatch::RequestContext` out of call sites.
pub use plane_core::context::RequestContext as DrfRequestContext;

#[cfg(test)]
mod tests {
    use super::*;
    use plane_permissions::PermissionClass;
    use plane_throttle::ThrottleClass;

    fn table() -> PolicyTable {
        PolicyTable::load().expect("routes.json")
    }

    /// `plane/app/views/base.py:182-198`: a mixin-served action with only
    /// `IsAuthenticated` is refused; the same action with a restrictive class,
    /// or one the view implements, is not.
    #[test]
    fn guard_refuses_mixin_actions_with_non_authorizing_permissions() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| {
                route.view == "plane.app.views.cycle.issue.CycleIssueViewSet"
                    && route
                        .methods
                        .iter()
                        .any(|entry| entry.action.as_deref() == Some("retrieve"))
            })
            .expect("cycle issue detail");
        assert!(guard_denies(
            route,
            &Method::GET,
            &[PermissionClass::IsAuthenticated]
        ));
        assert!(!guard_denies(
            route,
            &Method::GET,
            &[PermissionClass::AppProjectEntityPermission]
        ));
        // OPTIONS resolves to the `metadata` action, which DRF answers.
        assert!(!guard_denies(
            route,
            &Method::OPTIONS,
            &[PermissionClass::IsAuthenticated]
        ));
    }

    /// A route whose action the view implements is not refused.
    #[test]
    fn guard_allows_project_implemented_actions() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| {
                route.view == "plane.app.views.issue.base.IssueViewSet"
                    && route
                        .methods
                        .iter()
                        .any(|entry| entry.action.as_deref() == Some("retrieve"))
            })
            .expect("issue detail");
        assert!(!guard_denies(
            route,
            &Method::GET,
            &[PermissionClass::IsAuthenticated]
        ));
    }

    /// Route 14 mounts `FileAssetEndpoint` with a restricted
    /// `http_method_names`, so its POST is a 405 even though the same class
    /// serves POST on route 13 (`plane/app/urls/asset.py:27-36`). The
    /// dispatcher's check (before the handler lookup) is what keeps it a 405
    /// instead of reaching the other route's handler.
    #[test]
    fn a_restricted_url_shape_refuses_the_verb_its_class_registers() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| {
                route.order == 14 && route.view == "plane.app.views.asset.base.FileAssetEndpoint"
            })
            .expect("route 14");
        assert!(!route.is_allowed_method(&Method::POST));
        assert!(route.is_allowed_method(&Method::GET));
        assert!(route.is_allowed_method(&Method::DELETE));
        // `View.head` copies `get`; OPTIONS is implicit on a DRF view.
        assert!(route.is_allowed_method(&Method::HEAD));
        assert!(route.is_allowed_method(&Method::OPTIONS));
        let response = render_api(metadata::method_not_allowed(route, &Method::POST));
        assert_eq!(response.status, 405);
        assert_eq!(
            std::str::from_utf8(&response.body).expect("utf8"),
            r#"{"detail":"Method \"POST\" not allowed."}"#
        );
    }

    /// `plane/api/views/base.py:123-136`: only `BaseAPIView` copies the two
    /// headers. `BaseViewSet` (invitations, stickies) and the v1 api-roots do
    /// not, so an `ApiKeyRateThrottle` resolved for them cannot leak headers a
    /// Django response would not carry.
    #[test]
    fn rate_limit_headers_are_copied_by_base_api_view_only() {
        assert!(copies_rate_limit_headers(Family::ApiApiView));
        assert!(!copies_rate_limit_headers(Family::ApiViewSet));
        assert!(!copies_rate_limit_headers(Family::DrfApiView));
        assert!(!copies_rate_limit_headers(Family::AppApiView));
        assert!(!copies_rate_limit_headers(Family::AppViewSet));
        assert!(!copies_rate_limit_headers(Family::DjangoView));
        // And the v1 `BaseViewSet` rows really are throttled by the default.
        let table = table();
        let stickies = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.api.views.sticky.StickyViewSet")
            .expect("stickies");
        assert_eq!(
            stickies.effective_throttles(Some("list")),
            &[ThrottleClass::Anon]
        );
    }

    #[test]
    fn a_re_raised_error_gets_no_finalize_headers() {
        // A DRF view without a family handler: a non-`APIException` is
        // re-raised, so Django's 500 page carries no `Allow`
        // (`rest_framework/views.py:453-485`).
        assert!(is_re_raised(
            Family::DrfApiView,
            &ViewError::Unknown("boom".to_string())
        ));
        assert!(is_re_raised(Family::DrfViewSet, &ViewError::Validation));
        // An `APIException` is converted by DRF's handler, then
        // `finalize_response` adds `Allow`.
        assert!(!is_re_raised(
            Family::DrfApiView,
            &ViewError::Api(ApiError::NotAuthenticated)
        ));
        // Plane's family bases implement `handle_exception`, so their 500 is a
        // JSON response that went through `finalize_response`.
        assert!(!is_re_raised(
            Family::AppApiView,
            &ViewError::Unknown("boom".to_string())
        ));
        assert!(!is_re_raised(Family::AppViewSet, &ViewError::Integrity));
    }

    #[test]
    fn method_not_allowed_keeps_the_allow_header() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("analytics");
        let error = metadata::method_not_allowed(route, &Method::POST);
        assert_eq!(error.status(), 405);
        assert_eq!(
            error.headers(),
            vec![("Allow", "GET, HEAD, OPTIONS".to_string())]
        );
        let response = render_api(error);
        assert_eq!(response.status, 405);
        assert_eq!(
            response.header("allow"),
            Some("GET, HEAD, OPTIONS"),
            "the error renderer carries Allow; finalize sets the same value"
        );
        assert_eq!(
            std::str::from_utf8(&response.body).expect("utf8"),
            r#"{"detail":"Method \"POST\" not allowed."}"#
        );
    }
}
