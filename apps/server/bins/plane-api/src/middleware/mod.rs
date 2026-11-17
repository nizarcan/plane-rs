// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Django-compatible request pipeline.
//!
//! HTTP_CONTRACT §1 is explicit that a `tower::ServiceBuilder` in `MIDDLEWARE`
//! order is *not* equivalent to Django: Django runs request phases 0→12 and
//! response phases 12→0, and wraps **each** middleware in
//! `convert_exception_to_response`. Which headers a response carries therefore
//! depends on which layer produced it:
//!
//! * a response created at layer *i* runs the response phases *i*→0 (e.g. the
//!   CORS preflight at 0, the 413 from the body limiter at 10);
//! * an exception raised in layer *i*'s request phase is converted by the
//!   wrapper *around* layer *i*, so layer *i*'s own response phase is skipped,
//!   layers above *i* never run, and layers *i-1*→0 process the error response
//!   (e.g. the `ALLOWED_HOSTS` 400 raised at 4);
//! * an exception from the view is converted by the innermost wrapper, inside
//!   layer 12, so every response phase runs.
//!
//! This module implements that machine directly: one `handle` function, one
//! request loop in Django's order, and one response loop that is told the index
//! it starts from. It is deliberately not a tower layer stack — the ordering is
//! the contract, and stating it once in code is clearer than encoding it as a
//! particular sequence of combinators.
//!
//! Layer indices and their status:
//!
//! | # | Middleware | Status |
//! | --- | --- | --- |
//! | 0 | `corsheaders.middleware.CorsMiddleware` | full |
//! | 1 | `django.middleware.security.SecurityMiddleware` | response side |
//! | 2 | `whitenoise.middleware.WhiteNoiseMiddleware` | pass-through |
//! | 3 | `SessionMiddleware` | full (store + cookie) |
//! | 4 | `django.middleware.common.CommonMiddleware` | full |
//! | 5 | `django.middleware.csrf.CsrfViewMiddleware` | full (token check + cookie) |
//! | 6 | `django.contrib.auth.middleware.AuthenticationMiddleware` | lazy `request.user` |
//! | 7 | `django.middleware.clickjacking.XFrameOptionsMiddleware` | full |
//! | 8 | `crum.CurrentRequestUserMiddleware` | no-op (explicit user argument) |
//! | 9 | `django.middleware.gzip.GZipMiddleware` | full |
//! | 10 | `RequestBodySizeLimitMiddleware` | full |
//! | 11 | `APITokenLogMiddleware` | full (`process_logs` published, failures swallowed) |
//! | 12 | `RequestLoggerMiddleware` | full (and evaluates the lazy user) |
//!
//! Layer 6 has no request-phase work in Django either: it installs a
//! `SimpleLazyObject`. The Rust equivalent is [`auth::AuthContext`], which
//! evaluates [`auth::AuthContext::resolve`] the first time something reads the
//! user — today that is layer 12's logger and the `/_plane_rs/test/` views.

pub mod auth;
pub mod body_limit;
pub mod common;
pub mod cors;
pub mod csrf;
pub mod extensions;
pub mod gzip;
pub mod logger;
pub mod security;
pub mod session;
pub mod xframe;

use std::net::SocketAddr;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::Method;
use axum::response::Response;
use percent_encoding::percent_decode_str;
use plane_auth::AuthError;
use plane_core::context::RequestContext;
use plane_core::error::{ApiError, ERROR_PAGE_500, TEXT_HTML};

use crate::app::{AppState, SharedState};
use crate::handlers;
use crate::response::Resp;
use crate::routes::Handler;
use crate::routes::Route;

/// Layer indices, so the response loop reads in Django's numbering.
const LAYER_CORS: usize = 0;
const LAYER_SESSION: usize = 3;
const LAYER_COMMON: usize = 4;
const LAYER_CSRF: usize = 5;
const LAYER_XFRAME: usize = 7;
const LAYER_GZIP: usize = 9;
const LAYER_TOKEN_LOG: usize = 11;
const LAYER_LOGGER: usize = 12;

/// The axum fallback handler: every request goes through the pipeline.
pub async fn handle(State(state): State<SharedState>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let peer_addr = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    let path_info = decode_path(parts.uri.path());
    let query_string = parts.uri.query().unwrap_or("").to_string();
    let mut ctx = RequestContext::new(
        parts.method,
        path_info,
        query_string,
        parts.headers,
        peer_addr,
    );
    let method = ctx.method.clone();
    let mut body = Some(body);

    let resp = run(&state, &mut ctx, &mut body).await;
    resp.into_response(&method)
}

/// Django's `path_info`: percent-decoded, which is what the ASGI server puts in
/// `scope["path"]` (`django/core/handlers/asgi.py:54-60`).
fn decode_path(path: &str) -> String {
    percent_decode_str(path).decode_utf8_lossy().into_owned()
}

/// Request phases 0→12, routing, the view, then response phases 12→0.
async fn run(state: &AppState, ctx: &mut RequestContext, body: &mut Option<Body>) -> Resp {
    // 0. CorsMiddleware: a preflight is answered before anything else runs.
    if let Some(resp) = cors::request(ctx) {
        let mut db = auth::RequestDb::default();
        let mut auth_context = auth::AuthContext::new(&state.settings, ctx);
        return run_response_phases(
            state,
            ctx,
            &mut auth_context,
            &mut db,
            resp,
            LAYER_CORS,
            Instant::now(),
        )
        .await;
    }

    // 1. SecurityMiddleware.request: SSL redirect is off; HSTS is response-side.
    // 2. WhiteNoise: no request-phase effect.
    // 3. SessionMiddleware.process_request: choose the cookie by path and build
    //    the store. Nothing is read from the database yet.
    let mut db = auth::RequestDb::default();
    let mut auth_context = auth::AuthContext::new(&state.settings, ctx);

    // 4. CommonMiddleware: DISALLOWED_USER_AGENTS (empty) then get_host().
    if let Err(error) = common::request(&state.settings, ctx) {
        // The exception is converted by the wrapper *around* layer 4: layer 4's
        // own response phase (Content-Length) never runs.
        return run_error(
            state,
            ctx,
            &mut auth_context,
            &mut db,
            error,
            LAYER_COMMON,
            Instant::now(),
        )
        .await;
    }

    // 5. CsrfViewMiddleware.process_request: adopt or replace the cookie
    //    secret. `get_token`/`rotate_token` (layer > 5) update this state and
    //    layer 5's response phase writes the cookie.
    csrf::request(&state.settings, ctx, &auth_context.csrf).await;

    // 6-9: Authentication is lazy (see `auth::AuthContext`); XFrame and GZip
    // have no request-phase effect.

    // 10. RequestBodySizeLimitMiddleware reads the body once.
    if let Err(error) = body_limit::request(&state.settings, ctx, body).await {
        // A returned 413, not an exception: its own (empty) response phase runs,
        // so it is processed by layers 10→0 and not by the logger.
        return run_response_phases(
            state,
            ctx,
            &mut auth_context,
            &mut db,
            render(error),
            10,
            Instant::now(),
        )
        .await;
    }

    // `RequestLoggerMiddleware.__call__` starts its timer here: layer 10 already
    // read the body, so the logged duration excludes the body read, exactly like
    // Django's innermost middleware.
    let started = Instant::now();

    // `_get_response`: resolve, process_view hooks, then call the view.
    let route = state.routes.resolve(&ctx.path_info);
    ctx.csrf_exempt = route.map(|route| route.csrf_exempt).unwrap_or(false);
    // process_view hooks run in 0→12 order. Only CsrfViewMiddleware has one in
    // Plane: it checks the token for non-exempt views and returns the failure
    // page (HTTP 200) itself.
    let matched = route.is_some() || is_test_path(ctx);
    if let Some(rejection) = csrf::process_view(&state.settings, ctx, matched) {
        return run_response_phases(
            state,
            ctx,
            &mut auth_context,
            &mut db,
            rejection,
            LAYER_LOGGER,
            started,
        )
        .await;
    }

    let resp = dispatch_view(state, ctx, &mut auth_context, &mut db, route).await;
    run_response_phases(
        state,
        ctx,
        &mut auth_context,
        &mut db,
        resp,
        LAYER_LOGGER,
        started,
    )
    .await
}

/// Whether the request has a view at all. The CSRF `process_view` hook only
/// runs when the URL resolver matched; `/_plane_rs/test/` routes are matched too
/// when the feature is on, and they are **not** `csrf_exempt` — the exit check
/// proves a Django-issued token is accepted there.
#[cfg(feature = "test-endpoints")]
fn is_test_path(ctx: &RequestContext) -> bool {
    crate::test_endpoints::route(&ctx.path_info, ctx.method.clone()).is_some()
}

#[cfg(not(feature = "test-endpoints"))]
fn is_test_path(_ctx: &RequestContext) -> bool {
    false
}

/// Run the matched view.
///
/// `state`/`ctx`/`auth_context`/`db` are only read by the `test-endpoints`
/// build.
#[cfg_attr(not(feature = "test-endpoints"), allow(unused_variables))]
async fn dispatch_view(
    state: &AppState,
    ctx: &mut RequestContext,
    auth_context: &mut auth::AuthContext,
    db: &mut auth::RequestDb,
    route: Option<&Route>,
) -> Resp {
    #[cfg(feature = "test-endpoints")]
    {
        if let Some(test_route) = crate::test_endpoints::route(&ctx.path_info, ctx.method.clone()) {
            return crate::test_endpoints::handle(state, ctx, auth_context, db, test_route).await;
        }
    }
    match route {
        // `resolve` raised Resolver404: handler404 = custom_404_view.
        None => render(ApiError::PageNotFound),
        // A route the DRF runtime has a handler for goes through the full
        // policy. Everything else keeps Phase 1's answer.
        Some(route) if state.drf.is_registered(route.order, &ctx.method) => {
            dispatch_drf(state, ctx, auth_context, route).await
        }
        Some(route) => match route.handler {
            Handler::Health => handlers::health(),
            Handler::Robots => handlers::robots(),
            // A route Django serves that this binary has not ported. The proxy
            // keeps those paths on Django; if one arrives here anyway the answer
            // is an explicit 501 rather than a wrong success.
            Handler::Unported => render(ApiError::NotImplemented),
        },
    }
}

/// Run a registered view through `plane-drf`.
///
/// The runtime owns authentication, permissions, throttles, the family
/// `initial` hooks, `allow_permission` and the handler. What comes back is a
/// rendered response plus the two pieces of state the pipeline owns:
///
/// * which Django user `request._request.user` is now (lazy, replaced by
///   `_not_authenticated`, or resolved), because the logger at layer 12 and the
///   `Vary: Cookie` decision depend on it (HTTP_CONTRACT §1.1);
/// * `request.session.accessed`.
async fn dispatch_drf(
    state: &AppState,
    ctx: &mut RequestContext,
    auth_context: &mut auth::AuthContext,
    route: &Route,
) -> Resp {
    let kwargs = state.routes.kwargs(route, &ctx.path_info);
    let preloaded_user = auth_context.resolved.then(|| auth_context.user.clone());
    let outcome = require_send(state.drf.dispatch(
        &state.settings,
        &state.db,
        plane_drf::dispatch::DispatchRequest {
            order: route.order,
            kwargs,
            ctx,
            session: &auth_context.session,
            csrf: Some(&auth_context.csrf),
            preloaded_user,
        },
    ))
    .await;

    if outcome.session_accessed {
        ctx.session_accessed = true;
    }
    match outcome.django_user {
        plane_drf::UserOutcome::Lazy => {}
        plane_drf::UserOutcome::Anonymous => {
            auth_context.user = plane_auth::RequestUser::Anonymous;
            auth_context.resolved = true;
            auth_context.sync(ctx).await;
        }
        plane_drf::UserOutcome::Resolved(user) => {
            auth_context.user = user;
            auth_context.resolved = true;
            auth_context.sync(ctx).await;
        }
    }
    Resp::from_drf(outcome.response)
}

/// Compile-time proof that the DRF dispatch future is `Send`: axum's handler
/// requires it, and a regression here would otherwise only show up as an
/// unhelpful `Handler` bound error at the router.
fn require_send<F: std::future::Future + Send>(future: F) -> F {
    future
}

/// Render an error body with the right dialect. A rendering failure falls back
/// to Django's 500 page rather than panicking on a request path.
fn render(error: ApiError) -> Resp {
    match error.render() {
        Ok(response) => Resp::from_error_response(response),
        Err(error) => {
            tracing::error!(target: "plane.api", %error, "failed to render an error response");
            server_error()
        }
    }
}

fn server_error() -> Resp {
    Resp::new(500, TEXT_HTML, ERROR_PAGE_500.as_bytes().to_vec())
}

/// Run the response phases from `from` down to 0.
///
/// An error raised *inside* a response phase is converted by the wrapper around
/// that layer (Django's `convert_exception_to_response`), so the layer's own
/// work is abandoned and the remaining lower layers still run — hence the
/// `match` inside the loop rather than a `?`.
async fn run_response_phases(
    state: &AppState,
    ctx: &mut RequestContext,
    auth_context: &mut auth::AuthContext,
    db: &mut auth::RequestDb,
    resp: Resp,
    from: usize,
    started: Instant,
) -> Resp {
    let mut resp = resp;
    for index in (0..=from).rev() {
        match response_phase(state, ctx, auth_context, db, resp, index, started).await {
            Ok(next) => resp = next,
            Err(error) => {
                tracing::error!(target: "plane.api", %error, layer = index, "middleware failed");
                resp = render(ApiError::InternalServerError);
            }
        }
    }
    resp
}

/// An exception raised in request phase `thrown_at` is converted by the wrapper
/// around it, so that layer's own response phase is skipped.
async fn run_error(
    state: &AppState,
    ctx: &mut RequestContext,
    auth_context: &mut auth::AuthContext,
    db: &mut auth::RequestDb,
    error: ApiError,
    thrown_at: usize,
    started: Instant,
) -> Resp {
    let resp = render(error);
    if thrown_at == 0 {
        return resp;
    }
    run_response_phases(state, ctx, auth_context, db, resp, thrown_at - 1, started).await
}

/// One response phase, in Django's numbering.
async fn response_phase(
    state: &AppState,
    ctx: &mut RequestContext,
    auth_context: &mut auth::AuthContext,
    db: &mut auth::RequestDb,
    resp: Resp,
    index: usize,
    started: Instant,
) -> Result<Resp, AuthError> {
    match index {
        LAYER_CORS => Ok(cors::response(&state.settings, ctx, resp)),
        // 1. SecurityMiddleware.response.
        1 => Ok(security::response(ctx, resp)),
        LAYER_SESSION => session::response(state, ctx, auth_context, db, resp).await,
        LAYER_COMMON => Ok(common::response(&state.settings, &state.routes, ctx, resp)),
        LAYER_CSRF => Ok(csrf::response(&state.settings, &auth_context.csrf, resp).await),
        LAYER_XFRAME => Ok(xframe::response(resp)),
        LAYER_GZIP => Ok(gzip::response(ctx, resp)),
        LAYER_TOKEN_LOG => {
            extensions::api_token_log_response(&state.settings, ctx, &resp, state.drf.celery())
                .await;
            Ok(resp)
        }
        LAYER_LOGGER => {
            // `RequestLoggerMiddleware.process_response` reads
            // `request.user.is_authenticated` for every route except `GET /`
            // (`plane/middleware/logger.py:35-36,56-58`); that read is what
            // evaluates Django's lazy user and loads the session. Doing it for
            // `GET /` too would add a `Vary: Cookie` Django does not send
            // (HTTP_CONTRACT §1.1).
            if !(ctx.method == Method::GET && ctx.path == "/") {
                auth_context.resolve(state, ctx, db).await?;
            }
            logger::response(ctx, &resp, started);
            Ok(resp)
        }
        // 2 WhiteNoise, 6 Authentication, 8 crum, 10 body limit: no response
        // effect.
        _ => Ok(resp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, Method, StatusCode};
    use plane_core::config::{EnvMap, Role, Settings};
    use plane_db::DbPools;
    use std::sync::Arc;

    use crate::routes::RouteTable;

    fn state(database_url: &str, file_size_limit: &str) -> SharedState {
        let env = EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("DATABASE_URL", database_url)
            .set("FILE_SIZE_LIMIT", file_size_limit)
            .set("ALLOWED_HOSTS", "*")
            .set("WEB_URL", "http://127.0.0.1:18080");
        let settings = Settings::from_source(&env, Role::Api).expect("settings");
        state_from_settings(&settings)
    }

    fn state_from_settings(settings: &Settings) -> SharedState {
        let db = DbPools::from_settings(settings).expect("pools");
        let policy = plane_drf::policy::PolicyTable::load().expect("drf policy");
        let registry = plane_drf::registry::Registry::new(policy)
            .expect("registry")
            .finish()
            .expect("finish");
        let drf = plane_drf::dispatch::Runtime::new(registry, None, None).expect("runtime");
        Arc::new(AppState {
            settings: settings.clone(),
            routes: RouteTable::load().expect("route table"),
            db,
            drf,
        })
    }

    /// A state whose Postgres URL cannot be reached, for the tests that must
    /// prove a request never checks a connection out.
    pub(crate) fn state_without_database(settings: &Settings) -> SharedState {
        state_from_settings(settings)
    }

    async fn call(
        state: SharedState,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Body,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut builder = axum::http::Request::builder().method(method).uri(path);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(body).expect("request");
        let response = handle(State(state), request).await;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body")
            .to_vec();
        (status, headers, bytes)
    }

    fn headers(map: &HeaderMap, name: &str) -> Option<String> {
        map.get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    /// The health check must answer with Postgres unreachable: the pools are
    /// lazy, the session is never touched, and `/` never checks one out.
    #[tokio::test]
    async fn health_does_not_touch_the_database() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, header_map, body) = call(
            state,
            Method::GET,
            "/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"{\"status\": \"OK\"}");
        assert_eq!(
            headers(&header_map, "content-type"),
            Some("application/json".into())
        );
        assert_eq!(headers(&header_map, "content-length"), Some("16".into()));
        assert_eq!(headers(&header_map, "vary"), Some("origin".into()));
        assert!(headers(&header_map, "x-frame-options").is_some());
    }

    /// HTTP_CONTRACT §1.1: `GET /` is the one route the logger skips, so it is
    /// the one response without `Vary: Cookie`.
    #[tokio::test]
    async fn vary_cookie_matches_the_logged_routes() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (_, header_map, _) = call(
            Arc::clone(&state),
            Method::HEAD,
            "/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        assert_eq!(headers(&header_map, "vary"), Some("Cookie, origin".into()));

        let (_, header_map, _) = call(
            Arc::clone(&state),
            Method::GET,
            "/nope/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        assert_eq!(headers(&header_map, "vary"), Some("Cookie, origin".into()));
    }

    /// An anonymous `HEAD /` evaluates `request.user` without a session key, so
    /// it must not open a connection even though the logger runs.
    #[tokio::test]
    async fn anonymous_user_does_not_touch_the_database() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, header_map, _) = call(
            Arc::clone(&state),
            Method::HEAD,
            "/api/users/me/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        // The route is unported (501), and the logger still ran.
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(headers(&header_map, "vary"), Some("Cookie, origin".into()));
    }

    /// The 413 from the body limit is produced at layer 10, so the logger and
    /// therefore `Vary: Cookie` never run (HTTP_CONTRACT §9.1, §A.1.11).
    #[tokio::test]
    async fn request_body_limit_short_circuits_below_the_logger() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "10");
        let (status, header_map, body) = call(
            state,
            Method::POST,
            "/",
            &[
                ("host", "127.0.0.1:18101"),
                ("content-length", "11"),
                ("content-type", "text/plain"),
            ],
            Body::from(vec![b'a'; 11]),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body.len(), 111);
        assert_eq!(headers(&header_map, "vary"), Some("origin".into()));
        assert_eq!(headers(&header_map, "content-length"), Some("111".into()));
        assert_eq!(headers(&header_map, "x-frame-options"), Some("DENY".into()));
    }

    /// The `ALLOWED_HOSTS` 400 is raised at layer 4, so it gets the security
    /// headers of layers 3→0 but neither `X-Frame-Options` nor a
    /// `Content-Length` (HTTP_CONTRACT §11.3, §A.1.12).
    #[tokio::test]
    async fn disallowed_host_skips_the_layers_above_common() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, header_map, body) = call(
            state,
            Method::GET,
            "/",
            &[("host", "bad_host")],
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.len(), 143);
        assert!(headers(&header_map, "x-frame-options").is_none());
        assert!(headers(&header_map, "content-length").is_none());
        assert_eq!(headers(&header_map, "vary"), Some("origin".into()));
        assert_eq!(
            headers(&header_map, "x-content-type-options"),
            Some("nosniff".into())
        );
    }

    /// A CORS preflight is answered by layer 0 before routing, so it carries
    /// no security headers at all (HTTP_CONTRACT §A.1.3).
    #[tokio::test]
    async fn preflight_is_answered_by_the_first_layer() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, header_map, body) = call(
            state,
            Method::OPTIONS,
            "/anything/",
            &[
                ("host", "127.0.0.1:18101"),
                ("origin", "http://localhost:3000"),
                ("access-control-request-method", "GET"),
            ],
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_empty());
        assert_eq!(headers(&header_map, "content-length"), Some("0".into()));
        assert_eq!(headers(&header_map, "vary"), Some("origin".into()));
        assert_eq!(
            headers(&header_map, "access-control-allow-origin"),
            Some("http://localhost:3000".into())
        );
        assert!(headers(&header_map, "x-frame-options").is_none());
    }

    /// A path Django serves but Phase 1 has not ported answers 501 with a DRF
    /// body (our own marker; the proxy keeps those routes on Django).
    #[tokio::test]
    async fn unported_routes_answer_501() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, header_map, body) = call(
            state,
            Method::GET,
            "/api/instances/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body, b"{\"detail\":\"Not implemented by plane-api.\"}");
        assert_eq!(headers(&header_map, "vary"), Some("Cookie, origin".into()));
    }

    /// An unknown path gets Django's custom 404 in the Django JSON dialect.
    #[tokio::test]
    async fn unknown_paths_answer_the_custom_404() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, _, body) = call(
            state,
            Method::GET,
            "/nope/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, b"{\"error\": \"Page not found.\"}");
        assert_eq!(body.len(), 28);
    }

    /// Unsafe methods on the two function views hit Django's CSRF check: a
    /// 200 HTML failure page, not a 403 (HTTP_CONTRACT §7.5).
    #[tokio::test]
    async fn post_to_health_is_a_csrf_failure() {
        let state = state("postgresql://plane:plane@127.0.0.1:1/plane", "5242880");
        let (status, header_map, body) = call(
            state,
            Method::POST,
            "/",
            &[("host", "127.0.0.1:18101")],
            Body::empty(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers(&header_map, "content-type"),
            Some("text/html; charset=utf-8".into())
        );
        let text = String::from_utf8(body).expect("utf8");
        assert!(text.contains("CSRF Verification Failed"));
        assert!(text.contains("href=\"http://127.0.0.1:18080\""));
        assert_eq!(
            headers(&header_map, "vary"),
            Some("Accept-Encoding, Cookie, origin".into())
        );
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(decode_path("/api/users/me"), "/api/users/me");
        assert_eq!(decode_path("/a%20b"), "/a b");
        assert_eq!(decode_path("/a%2Fb"), "/a/b");
        // Invalid UTF-8 is replaced, never a panic.
        assert_eq!(decode_path("/%FF"), "/�");
    }

    /// Compiled without the feature, the test endpoints must not exist: no
    /// route table entry, and no handler module. `cargo build --release` is
    /// also checked with `strings` in the Phase 2 report.
    #[cfg(not(feature = "test-endpoints"))]
    #[test]
    fn test_endpoints_are_absent_by_default() {
        const { assert!(!cfg!(feature = "test-endpoints")) };
        assert!(
            RouteTable::load()
                .expect("route table")
                .resolve("/_plane_rs/test/whoami")
                .is_none()
        );
    }
}
