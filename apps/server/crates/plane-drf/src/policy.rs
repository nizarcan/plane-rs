// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The per-route policy table, generated from Django's own introspection.
//!
//! `apps/server/routes.json` is produced by `apps/server/tools/inventory/routes.py`
//! from the live URL resolver, so every value here is read off the Django view
//! class rather than transcribed. The table is embedded at compile time with
//! `include_str!` for the same reasons the Phase 1 route table does it:
//!
//! * one source of truth, no generated Rust to review or drift;
//! * `cargo` rebuilds when the JSON changes;
//! * a policy change shows up as a one-line diff of `routes.json` instead of a
//!   hand-edited table.
//!
//! Loading is strict. Every dotted class name must resolve to an enum variant
//! this crate can run (`plane_permissions::PermissionClass`,
//! `plane_throttle::ThrottleClass`, [`AuthClass`]), every lifecycle hook a
//! concrete view overrides must have a known implementation here, and any
//! `allow_permission` decorator whose arguments the inventory could not read is
//! a startup error. Nothing is silently skipped: a stale policy is a process
//! that does not start, not a route that runs with fewer checks.
//!
//! Python references (`plane-drf` reads them, the inventory records them):
//!
//! | Python | Here |
//! | --- | --- |
//! | `plane/app/views/base.py:67-366` | [`Family::AppViewSet`], [`Family::AppApiView`] |
//! | `plane/api/views/base.py:52-279` | [`Family::ApiViewSet`], [`Family::ApiApiView`] |
//! | `plane/space/views/base.py:62-225` | [`Family::SpaceViewSet`], [`Family::SpaceApiView`] |
//! | `plane/license/api/views/base.py:42-119` | [`Family::LicenseApiView`] |
//! | `rest_framework/views.py:490-538` | [`crate::dispatch`] |
//! | `rest_framework/viewsets.py:146-159` | [`RoutePolicy::action_for`] |

use std::collections::{BTreeMap, BTreeSet};

use http::Method;
use plane_core::ApiError;
use plane_permissions::{AllowPermission, CreatorModel, Level, PermissionClass, Role};
use plane_throttle::ThrottleClass;

use crate::error::ViewError;

/// The embedded inventory (`apps/server/routes.json`).
pub const ROUTES_JSON: &str = include_str!("../../../routes.json");

/// Django's `http_method_names` order (`django/views/generic/base.py:43-52`).
pub const HTTP_METHODS: [Method; 8] = [
    Method::GET,
    Method::POST,
    Method::PUT,
    Method::PATCH,
    Method::DELETE,
    Method::HEAD,
    Method::OPTIONS,
    Method::TRACE,
];

/// Lowercase name → method, for the inventory's JSON keys.
pub fn method_from_name(name: &str) -> Option<Method> {
    HTTP_METHODS
        .iter()
        .find(|method| method.as_str().eq_ignore_ascii_case(name))
        .cloned()
}

/// A policy problem found while loading `routes.json`. Each one aborts startup.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("routes.json is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("route {order} ({path}): unknown authentication class {class:?}")]
    UnknownAuthentication {
        order: u32,
        path: String,
        class: String,
    },
    #[error("route {order} ({path}): unknown permission class {class:?}")]
    UnknownPermission {
        order: u32,
        path: String,
        class: String,
    },
    #[error("route {order} ({path}): unknown throttle class {class:?}")]
    UnknownThrottle {
        order: u32,
        path: String,
        class: String,
    },
    #[error("route {order} ({path}): unknown base family {family:?}")]
    UnknownFamily {
        order: u32,
        path: String,
        family: String,
    },
    #[error(
        "route {order} ({path}): allow_permission on {method} has {what}; the policy cannot be \
         applied statically and the route needs an explicit implementation"
    )]
    UnresolvedAllowPermission {
        order: u32,
        path: String,
        method: String,
        what: String,
    },
    #[error(
        "view {view}: lifecycle hook {hook} is overridden by {owner}; plane-drf has no \
         implementation for that override"
    )]
    UnhandledLifecycleOverride {
        view: String,
        hook: &'static str,
        owner: String,
    },
    #[error(
        "view {view}: dynamic {kind} are declared by {owner}, but no implementation is \
         registered for this class"
    )]
    UnhandledDynamicPolicy {
        view: String,
        kind: &'static str,
        owner: String,
    },
    #[error("route {order} ({path}): {what}")]
    InvalidRoute {
        order: u32,
        path: String,
        what: String,
    },
}

// ---------------------------------------------------------------------------
// Families
// ---------------------------------------------------------------------------

/// Which base class a view derives from. Decides the exception mapping, the
/// extra `initial()` steps and the throttle defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    AppViewSet,
    AppApiView,
    ApiViewSet,
    ApiApiView,
    SpaceViewSet,
    SpaceApiView,
    LicenseApiView,
    DrfViewSet,
    DrfApiView,
    /// A plain `django.views.View` subclass.
    DjangoView,
    /// A function view.
    Function,
}

impl Family {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "app.BaseViewSet" => Self::AppViewSet,
            "app.BaseAPIView" => Self::AppApiView,
            "api.BaseViewSet" => Self::ApiViewSet,
            "api.BaseAPIView" => Self::ApiApiView,
            "space.BaseViewSet" => Self::SpaceViewSet,
            "space.BaseAPIView" => Self::SpaceApiView,
            "license.BaseAPIView" => Self::LicenseApiView,
            "drf.ViewSet" => Self::DrfViewSet,
            "drf.APIView" => Self::DrfApiView,
            "django.View" => Self::DjangoView,
            "function" => Self::Function,
            _ => return None,
        })
    }

    pub fn is_drf(self) -> bool {
        !matches!(self, Self::DjangoView | Self::Function)
    }

    pub fn is_viewset(self) -> bool {
        matches!(
            self,
            Self::AppViewSet | Self::ApiViewSet | Self::SpaceViewSet
        ) || self == Self::DrfViewSet
    }

    /// Whether `handle_exception` belongs to Plane's base class, so every
    /// escaped exception becomes a JSON body (§11.2 of HTTP_CONTRACT).
    pub fn has_family_exception_handler(self) -> bool {
        matches!(
            self,
            Self::AppViewSet
                | Self::AppApiView
                | Self::ApiViewSet
                | Self::ApiApiView
                | Self::SpaceViewSet
                | Self::SpaceApiView
                | Self::LicenseApiView
        )
    }

    /// The 404 wording of `ObjectDoesNotExist` (`plane/api/views/base.py:95`
    /// is the only family that says "requested resource").
    pub fn object_does_not_exist(self) -> ApiError {
        if self == Self::ApiApiView {
            ApiError::ResourceDoesNotExist
        } else {
            ApiError::ObjectDoesNotExist
        }
    }

    /// The 500 body Plane's `handle_exception` returns for an uncaught error.
    pub fn uncaught(self) -> ApiError {
        if self.has_family_exception_handler() {
            ApiError::FamilyServerError
        } else {
            // `rest_framework.views.APIView.dispatch` re-raises, so Django's
            // handler answers with its 500 page; a plain view never reaches
            // DRF at all.
            ApiError::InternalServerError
        }
    }

    /// The extra `initial()` step this family adds after
    /// `APIView.initial()` and `TimezoneMixin.initial()`.
    pub fn initial_extra(self) -> InitialExtra {
        match self {
            Self::AppViewSet => InitialExtra::ViewSetGuard,
            Self::ApiApiView | Self::ApiViewSet => InitialExtra::BodyMustBeContainer,
            Self::SpaceViewSet | Self::SpaceApiView => InitialExtra::DeployBoardAnchor,
            _ => InitialExtra::None,
        }
    }

    /// The exception mapping `plane/*/views/base.py` implements.
    pub fn exception(self, error: ViewError) -> ApiError {
        if !self.is_drf() {
            return match error {
                ViewError::Api(error) => error,
                _ => ApiError::InternalServerError,
            };
        }
        if !self.has_family_exception_handler() {
            // A plain DRF view only maps `APIException` subclasses; everything
            // else is re-raised and Django answers with the 500 page.
            return match error {
                ViewError::Api(error) => error,
                _ => ApiError::InternalServerError,
            };
        }
        match error {
            // Already mapped by `rest_framework.views.exception_handler` or by
            // `plane/authentication/adapter/exception.py`.
            ViewError::Api(error) => error,
            ViewError::Integrity => ApiError::InvalidPayload,
            ViewError::Validation => ApiError::InvalidDetail,
            ViewError::ObjectDoesNotExist => self.object_does_not_exist(),
            ViewError::Key => ApiError::MissingKey,
            // Django's `TooManyFieldsSent` is a `SuspiciousOperation`; the
            // family handler catches it like any other exception.
            ViewError::TooManyFields => ApiError::FamilyServerError,
            ViewError::Unknown(_) => ApiError::FamilyServerError,
        }
    }
}

/// The family-specific `initial()` step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialExtra {
    None,
    /// `plane/app/views/base.py:174-198`.
    ViewSetGuard,
    /// `plane/api/views/base.py:59-62,171-174`.
    BodyMustBeContainer,
    /// `plane/space/views/base.py:47-59`.
    DeployBoardAnchor,
}

// ---------------------------------------------------------------------------
// Security classes
// ---------------------------------------------------------------------------

/// One configured DRF authenticator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthClass {
    /// `plane.authentication.session.BaseSessionAuthentication`.
    BaseSession,
    /// `plane.api.middleware.api_authentication.APIKeyAuthentication`.
    ApiKey,
    /// Plain `rest_framework.authentication.SessionAuthentication`, which runs
    /// Django's full CSRF token check on authenticated callers.
    DrfSession,
}

impl AuthClass {
    pub fn from_dotted(path: &str) -> Option<Self> {
        match path {
            "plane.authentication.session.BaseSessionAuthentication" => Some(Self::BaseSession),
            "plane.api.middleware.api_authentication.APIKeyAuthentication" => Some(Self::ApiKey),
            "rest_framework.authentication.SessionAuthentication" => Some(Self::DrfSession),
            _ => None,
        }
    }

    pub fn dotted(self) -> &'static str {
        match self {
            Self::BaseSession => "plane.authentication.session.BaseSessionAuthentication",
            Self::ApiKey => "plane.api.middleware.api_authentication.APIKeyAuthentication",
            Self::DrfSession => "rest_framework.authentication.SessionAuthentication",
        }
    }

    /// `authenticate_header(request)`. Both Plane classes inherit
    /// `BaseAuthentication.authenticate_header`, which returns `None`, so a
    /// failed authentication is 403; DRF's `SessionAuthentication` also
    /// returns `None`.
    pub fn authenticate_header(self) -> Option<&'static str> {
        None
    }
}

/// `UserEndpoint.get_throttles` (`plane/app/views/user/base.py:71-80`), the only
/// concrete `get_throttles` override in the tree that is not a family base.
pub fn user_endpoint_throttles(action: Option<&str>) -> Option<&'static [ThrottleClass]> {
    const CODE: &[ThrottleClass] = &[
        ThrottleClass::EmailVerification,
        ThrottleClass::Authentication,
    ];
    const UPDATE_EMAIL: &[ThrottleClass] = &[ThrottleClass::Authentication];
    match action {
        Some("generate_email_verification_code") => Some(CODE),
        Some("update_email") => Some(UPDATE_EMAIL),
        _ => None,
    }
}

/// `plane_permissions::dynamic_permission_classes` covers every
/// `get_permissions` override; this exists so the policy loader can name the
/// implementation in an error message.
pub fn dynamic_permissions_known(view: &str) -> bool {
    plane_permissions::dynamic_permission_classes(view, &Method::GET, None).is_some()
        || plane_permissions::dynamic::DYNAMIC_VIEWS.contains(&view)
        || plane_permissions::dynamic::SUBCLASS_ALIASES
            .iter()
            .any(|(alias, _)| *alias == view)
}

/// `issue_hidden_from_guest` calls inside `initial()`: the two app viewsets
/// that raise `PermissionDenied("You are not allowed to view this issue")`
/// before the action runs.
pub fn initial_issue_hidden_from_guest(view: &str) -> bool {
    matches!(
        view,
        "plane.app.views.issue.link.IssueLinkViewSet"
            | "plane.app.views.issue.subscriber.IssueSubscriberViewSet"
    )
}

// ---------------------------------------------------------------------------
// routes.json shape
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct RoutesFile {
    routes: Vec<RawRoute>,
}

#[derive(Debug, serde::Deserialize)]
struct RawRoute {
    order: u32,
    #[serde(default)]
    group: String,
    path: String,
    django_pattern: String,
    #[serde(default)]
    name: Option<String>,
    view: String,
    kind: String,
    #[serde(default)]
    methods: BTreeMap<String, String>,
    #[serde(default)]
    implicit_methods: Vec<String>,
    #[serde(default)]
    authentication_classes: Vec<String>,
    #[serde(default)]
    permission_classes: Vec<String>,
    #[serde(default)]
    throttle_classes: Vec<String>,
    #[serde(default)]
    method_permissions: BTreeMap<String, Vec<RawAllowPermission>>,
    #[serde(default)]
    method_throttles: BTreeMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    dynamic_authenticators: bool,
    #[serde(default)]
    dynamic_permissions: bool,
    #[serde(default)]
    dynamic_throttles: bool,
    #[serde(default)]
    viewset_guard: RawGuard,
    #[serde(default)]
    guard_actions: BTreeMap<String, RawGuardAction>,
    #[serde(default)]
    base_family: String,
    #[serde(default)]
    use_read_replica: Option<bool>,
    #[serde(default)]
    timezone_mixin: bool,
    #[serde(default)]
    http_method_names: Option<Vec<String>>,
    #[serde(default)]
    renderer_classes: Option<Vec<String>>,
    #[serde(default)]
    parser_classes: Option<Vec<String>>,
    #[serde(default)]
    content_negotiation_class: Option<String>,
    #[serde(default)]
    metadata_class: Option<String>,
    #[serde(default)]
    view_name: Option<String>,
    #[serde(default)]
    view_description: Option<String>,
    #[serde(default)]
    lifecycle_overrides: BTreeMap<String, Option<String>>,
    #[serde(default)]
    options_metadata: RawOptions,
}

#[derive(Debug, Default, serde::Deserialize)]
struct RawGuard {
    #[serde(default)]
    refused_methods: Vec<RawRefusedMethod>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct RawGuardAction {
    #[serde(default)]
    owner: String,
    #[serde(default)]
    mixin_provided: bool,
    #[serde(default)]
    project_implemented: bool,
    #[serde(default)]
    project_implements_create: bool,
    #[serde(default)]
    project_implements_update: bool,
}

#[derive(Debug, serde::Deserialize)]
struct RawRefusedMethod {
    method: String,
}

#[derive(Debug, Default, serde::Deserialize)]
struct RawOptions {
    #[serde(default)]
    attempts: Vec<String>,
    #[serde(default)]
    actions_json: BTreeMap<String, String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    get_object_required: bool,
}

#[derive(Debug, serde::Deserialize)]
struct RawAllowPermission {
    #[serde(default)]
    unknown: bool,
    #[serde(default)]
    allowed_roles: Option<Vec<String>>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    creator: bool,
    #[serde(default)]
    model: Option<String>,
}

// ---------------------------------------------------------------------------
// The resolved policy
// ---------------------------------------------------------------------------

/// One `@allow_permission(...)` decorator, resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowRule {
    pub permission: AllowPermission,
}

/// One registered HTTP method of one route.
#[derive(Debug, Clone)]
pub struct MethodPolicy {
    pub method: Method,
    /// The ViewSet action or the handler method name (`routes.json` `methods`).
    pub action: Option<String>,
    /// The `allow_permission` decorator for this method, when it has one.
    pub allow: Option<AllowRule>,
    /// Throttles the decorator itself adds (only the auth-flow redirect
    /// decorator today, which never appears on a DRF route).
    pub decorator_throttles: Vec<serde_json::Value>,
    /// `BaseViewSet.initial` refuses this method statically.
    pub refused_by_guard: bool,
}

/// The action half of `BaseViewSet._resolved_action_is_authorized`
/// (`plane/app/views/base.py:103-172`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardAction {
    pub owner: String,
    pub mixin_provided: bool,
    pub project_implemented: bool,
    pub project_implements_create: bool,
    pub project_implements_update: bool,
}

/// `SimpleMetadata.determine_metadata`'s static half.
#[derive(Debug, Clone, Default)]
pub struct OptionsMetadata {
    /// `{'PUT', 'POST'} & set(view.allowed_methods)`, in POST/PUT order because
    /// `determine_actions` iterates a `set` whose iteration order CPython fixes
    /// for these two strings (`POST` first, then `PUT`).
    pub attempts: Vec<Method>,
    /// `actions_json[method]`: the serializer field map as compact
    /// `json.dumps` text, order preserved.
    pub actions_json: BTreeMap<String, String>,
    /// `determine_actions` raised for this view; Django answers the OPTIONS
    /// with the family 500 body.
    pub error: Option<String>,
    pub get_object_required: bool,
}

/// One resolver entry, with everything the runtime needs.
#[derive(Debug, Clone)]
pub struct RoutePolicy {
    pub order: u32,
    pub group: String,
    pub path_template: String,
    pub django_pattern: String,
    pub url_name: Option<String>,
    pub view: String,
    pub kind: String,
    pub family: Family,
    pub methods: Vec<MethodPolicy>,
    /// `head` and/or `options` the view answers without a registered handler.
    pub implicit_methods: Vec<Method>,
    /// `as_view(http_method_names=[...])`'s list (or Django's default). Django's
    /// `dispatch` asks `method.lower() in self.http_method_names` before it
    /// looks for a handler, so a verb outside the list is a 405 even when the
    /// class implements it for another URL shape.
    pub http_method_names: Vec<Method>,
    /// The precomputed `Allow` header (`APIView.default_response_headers`).
    /// `None` for a function view, which never restricts methods.
    pub allow: Option<String>,
    pub use_read_replica: bool,
    pub timezone_mixin: bool,
    pub renderer_classes: Vec<String>,
    pub parser_classes: Vec<String>,
    pub content_negotiation_class: String,
    pub metadata_class: Option<String>,
    pub view_name: Option<String>,
    pub view_description: Option<String>,
    pub authentications: Vec<AuthClass>,
    pub permissions: Vec<PermissionClass>,
    pub throttles: Vec<ThrottleClass>,
    pub dynamic_permissions: bool,
    pub dynamic_throttles: bool,
    pub dynamic_authenticators: bool,
    pub guard_actions: BTreeMap<String, GuardAction>,
    pub options: OptionsMetadata,
    /// `initial()` must also run `issue_hidden_from_guest` before dispatch.
    pub initial_issue_hidden_from_guest: bool,
    /// `get_throttles` is the app `UserEndpoint` override.
    pub user_endpoint_throttles: bool,
}

impl RoutePolicy {
    /// Alias used by the request object.
    pub fn method_policy(&self, method: &Method) -> Option<&MethodPolicy> {
        self.method(method)
    }

    pub fn method(&self, method: &Method) -> Option<&MethodPolicy> {
        match self.methods.iter().find(|entry| entry.method == *method) {
            Some(entry) => Some(entry),
            // `ViewSetMixin.as_view`: `actions['head'] = actions['get']`.
            None if *method == Method::HEAD => self
                .methods
                .iter()
                .find(|entry| entry.method == Method::GET),
            None => None,
        }
    }

    /// The ViewSet action DRF's `initialize_request` sets for this request.
    ///
    /// `options` is special-cased to `metadata` (`rest_framework/viewsets.py:146-159`).
    pub fn action_for(&self, method: &Method) -> Option<&str> {
        if *method == Method::OPTIONS {
            return if self.family.is_viewset() {
                Some("metadata")
            } else {
                None
            };
        }
        self.method(method).and_then(|entry| {
            if self.family.is_viewset() {
                entry.action.as_deref()
            } else {
                None
            }
        })
    }

    /// Whether Django's method dispatch accepts this method at all.
    pub fn is_allowed_method(&self, method: &Method) -> bool {
        if !self.family.is_drf() && self.family != Family::DjangoView {
            // Function views accept every method.
            return true;
        }
        // `django/views/generic/base.py:134-144`: a verb outside
        // `http_method_names` goes straight to `http_method_not_allowed`.
        if !self.http_method_names.contains(method) {
            return false;
        }
        // DRF's `APIView` (and Django's `View`) define `options`; OPTIONS is
        // implicit on a DRF view.
        if *method == Method::OPTIONS {
            return self.implicit_methods.contains(&Method::OPTIONS);
        }
        // `View.setup` binds `head` to `get` when the class has no `head` of
        // its own (`django/views/generic/base.py:126-129`), so a row whose
        // `http_method_names` lists `head` answers HEAD whenever it registers
        // GET. A route built with an explicit `http_method_names` without
        // `head` (the v1 views) is still a 405 above.
        if *method == Method::HEAD {
            return self.methods.iter().any(|entry| entry.method == Method::GET);
        }
        self.methods.iter().any(|entry| entry.method == *method)
    }

    pub fn effective_throttles(&self, action: Option<&str>) -> &[ThrottleClass] {
        if self.user_endpoint_throttles
            && let Some(throttles) = user_endpoint_throttles(action)
        {
            return throttles;
        }
        &self.throttles
    }
}

/// Every route, indexed by resolver order, plus the per-view class policy.
#[derive(Debug)]
pub struct PolicyTable {
    routes: Vec<RoutePolicy>,
    by_order: BTreeMap<u32, usize>,
    views: BTreeMap<String, ViewPolicy>,
}

/// Class-level policy shared by every route of one view.
#[derive(Debug, Clone)]
pub struct ViewPolicy {
    pub view: String,
    pub family: Family,
    pub timezone_mixin: bool,
    pub use_read_replica: bool,
}

impl PolicyTable {
    /// Parse and validate the embedded inventory.
    pub fn load() -> Result<Self, PolicyError> {
        Self::from_json(ROUTES_JSON)
    }

    pub fn from_json(json: &str) -> Result<Self, PolicyError> {
        let file: RoutesFile = serde_json::from_str(json)?;
        let mut routes = Vec::with_capacity(file.routes.len());
        let mut by_order = BTreeMap::new();
        let mut views: BTreeMap<String, ViewPolicy> = BTreeMap::new();

        for raw in file.routes {
            let route = resolve_route(raw)?;
            if by_order.insert(route.order, routes.len()).is_some() {
                return Err(PolicyError::InvalidRoute {
                    order: route.order,
                    path: route.path_template.clone(),
                    what: "duplicate resolver order".to_string(),
                });
            }
            views
                .entry(route.view.clone())
                .or_insert_with(|| ViewPolicy {
                    view: route.view.clone(),
                    family: route.family,
                    timezone_mixin: route.timezone_mixin,
                    use_read_replica: route.use_read_replica,
                });
            routes.push(route);
        }

        Ok(Self {
            routes,
            by_order,
            views,
        })
    }

    /// The first-match resolver entry with this order.
    pub fn route(&self, order: u32) -> Option<&RoutePolicy> {
        self.by_order
            .get(&order)
            .and_then(|index| self.routes.get(*index))
    }

    pub fn routes(&self) -> &[RoutePolicy] {
        &self.routes
    }

    pub fn views(&self) -> &BTreeMap<String, ViewPolicy> {
        &self.views
    }

    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Every route that registers at least one method, as `(order, view, method)`.
    ///
    /// `HEAD` is included for a route with a `GET`, because `ViewSetMixin` (and
    /// Django's `View.setup`) bind it, and the proxy sends `HEAD` to the Rust
    /// owner of a `GET`.
    pub fn method_pairs(&self) -> Vec<(u32, String, Method)> {
        let mut pairs = Vec::new();
        for route in &self.routes {
            for entry in &route.methods {
                pairs.push((route.order, route.view.clone(), entry.method.clone()));
            }
            if route
                .methods
                .iter()
                .any(|entry| entry.method == Method::GET)
                && !route
                    .methods
                    .iter()
                    .any(|entry| entry.method == Method::HEAD)
            {
                pairs.push((route.order, route.view.clone(), Method::HEAD));
            }
        }
        pairs
    }
}

fn resolve_route(raw: RawRoute) -> Result<RoutePolicy, PolicyError> {
    let family = Family::from_name(&raw.base_family).ok_or_else(|| PolicyError::UnknownFamily {
        order: raw.order,
        path: raw.path.clone(),
        family: raw.base_family.clone(),
    })?;

    let mut authentications = Vec::with_capacity(raw.authentication_classes.len());
    for class in &raw.authentication_classes {
        authentications.push(AuthClass::from_dotted(class).ok_or_else(|| {
            PolicyError::UnknownAuthentication {
                order: raw.order,
                path: raw.path.clone(),
                class: class.clone(),
            }
        })?);
    }

    let mut permissions = Vec::with_capacity(raw.permission_classes.len());
    for class in &raw.permission_classes {
        permissions.push(PermissionClass::from_dotted(class).ok_or_else(|| {
            PolicyError::UnknownPermission {
                order: raw.order,
                path: raw.path.clone(),
                class: class.clone(),
            }
        })?);
    }

    let mut throttles = Vec::with_capacity(raw.throttle_classes.len());
    for class in &raw.throttle_classes {
        throttles.push(ThrottleClass::from_dotted(class).ok_or_else(|| {
            PolicyError::UnknownThrottle {
                order: raw.order,
                path: raw.path.clone(),
                class: class.clone(),
            }
        })?);
    }

    // `plane/api/views/base.py:69-70`: `BaseAPIView.get_throttles` returns
    // `[ApiKeyRateThrottle()]`, replacing the defaults. The inventory records
    // that as a `get_throttles` override owned by the family base.
    //
    // `BaseViewSet` (`:162-279`) overrides neither `get_throttles` nor
    // `throttle_classes`, so the `BaseViewSet` rows (invitations, stickies)
    // keep `DEFAULT_THROTTLE_CLASSES` = `AnonRateThrottle`
    // (`plane/settings/common.py:141`), which is a no-op for an authenticated
    // caller and emits no `X-RateLimit-*` headers (`HTTP_CONTRACT.md`
    // §12 "AnonRateThrottle"). The inventory records that class for them.
    let family_throttles = family == Family::ApiApiView;
    if family_throttles {
        throttles = vec![ThrottleClass::ApiKey];
    }
    let dynamic_throttles = raw.dynamic_throttles && !family_throttles;

    let mut methods = Vec::with_capacity(raw.methods.len());
    for (name, action) in &raw.methods {
        let method = method_from_name(name).ok_or_else(|| PolicyError::InvalidRoute {
            order: raw.order,
            path: raw.path.clone(),
            what: format!("unknown method {name:?} in `methods`"),
        })?;
        let allow = resolve_allow(raw.order, &raw.path, name, raw.method_permissions.get(name))?;
        let decorator_throttles = raw.method_throttles.get(name).cloned().unwrap_or_default();
        methods.push(MethodPolicy {
            method,
            action: Some(action.clone()),
            allow,
            decorator_throttles,
            refused_by_guard: false,
        });
    }
    methods.sort_by_key(|entry| {
        HTTP_METHODS
            .iter()
            .position(|method| *method == entry.method)
            .unwrap_or(usize::MAX)
    });

    let implicit_methods: Vec<Method> = raw
        .implicit_methods
        .iter()
        .filter_map(|name| method_from_name(name))
        .collect();

    let http_method_names: Vec<Method> = raw
        .http_method_names
        .as_ref()
        .map(|names| {
            names
                .iter()
                .filter_map(|name| method_from_name(name))
                .collect()
        })
        .unwrap_or_else(|| HTTP_METHODS.to_vec());

    // `APIView.default_response_headers` (`views.py:158-165`): every bound
    // handler in `http_method_names` order. Function views have no such list.
    let allow = if matches!(family, Family::Function) {
        None
    } else {
        let mut present: BTreeSet<Method> =
            methods.iter().map(|entry| entry.method.clone()).collect();
        present.extend(implicit_methods.iter().cloned());
        Some(
            http_method_names
                .iter()
                .filter(|method| present.contains(*method))
                .map(|method| method.as_str().to_uppercase())
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let refused: BTreeSet<Method> = raw
        .viewset_guard
        .refused_methods
        .iter()
        .filter_map(|entry| method_from_name(&entry.method))
        .collect();
    for entry in &mut methods {
        entry.refused_by_guard = refused.contains(&entry.method);
    }
    // `BaseViewSet.initial`'s action half: recorded for every action of the
    // view, independent of permissions, so the runtime can re-evaluate the rule
    // with whatever `get_permissions` resolved to.
    let guard_actions = raw
        .guard_actions
        .iter()
        .map(|(action, entry)| {
            (
                action.clone(),
                GuardAction {
                    owner: entry.owner.clone(),
                    mixin_provided: entry.mixin_provided,
                    project_implemented: entry.project_implemented,
                    project_implements_create: entry.project_implements_create,
                    project_implements_update: entry.project_implements_update,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut attempts = Vec::new();
    for name in &raw.options_metadata.attempts {
        if let Some(method) = method_from_name(name)
            && matches!(method, Method::POST | Method::PUT)
        {
            attempts.push(method);
        }
    }
    attempts.sort_by_key(|method| if *method == Method::POST { 0 } else { 1 });

    let lifecycle = &raw.lifecycle_overrides;
    let initial_issue_hidden_from_guest = raw
        .lifecycle_overrides
        .get("initial")
        .and_then(|owner| owner.clone())
        .is_some_and(|_| initial_issue_hidden_from_guest(&raw.view));
    let user_endpoint_throttles = raw
        .lifecycle_overrides
        .get("get_throttles")
        .and_then(|owner| owner.clone())
        .is_some_and(|_| true);
    let dynamic_permissions = raw.dynamic_permissions;
    if dynamic_permissions && !dynamic_permissions_known(&raw.view) {
        return Err(PolicyError::UnhandledDynamicPolicy {
            view: raw.view,
            kind: "permissions",
            owner: lifecycle
                .get("get_permissions")
                .and_then(|owner| owner.clone())
                .unwrap_or_default(),
        });
    }
    if dynamic_throttles && raw.view != "plane.app.views.user.base.UserEndpoint" {
        return Err(PolicyError::UnhandledDynamicPolicy {
            view: raw.view,
            kind: "throttles",
            owner: lifecycle
                .get("get_throttles")
                .and_then(|owner| owner.clone())
                .unwrap_or_default(),
        });
    }
    if raw.dynamic_authenticators {
        return Err(PolicyError::UnhandledDynamicPolicy {
            view: raw.view,
            kind: "authenticators",
            owner: lifecycle
                .get("get_authenticators")
                .and_then(|owner| owner.clone())
                .unwrap_or_default(),
        });
    }

    validate_lifecycle(&raw, family, dynamic_throttles)?;

    Ok(RoutePolicy {
        order: raw.order,
        group: raw.group,
        path_template: raw.path,
        django_pattern: raw.django_pattern,
        url_name: raw.name,
        view: raw.view,
        kind: raw.kind,
        family,
        methods,
        implicit_methods,
        allow,
        use_read_replica: raw.use_read_replica.unwrap_or(false),
        timezone_mixin: raw.timezone_mixin,
        renderer_classes: raw.renderer_classes.unwrap_or_default(),
        parser_classes: raw.parser_classes.unwrap_or_default(),
        content_negotiation_class: raw.content_negotiation_class.unwrap_or_default(),
        metadata_class: raw.metadata_class,
        view_name: raw.view_name,
        view_description: raw.view_description,
        authentications,
        permissions,
        throttles,
        dynamic_permissions,
        dynamic_throttles,
        dynamic_authenticators: raw.dynamic_authenticators,
        guard_actions,
        http_method_names,
        options: OptionsMetadata {
            attempts,
            actions_json: raw.options_metadata.actions_json,
            error: raw.options_metadata.error,
            get_object_required: raw.options_metadata.get_object_required,
        },
        initial_issue_hidden_from_guest,
        user_endpoint_throttles,
    })
}

fn resolve_allow(
    order: u32,
    path: &str,
    method: &str,
    entries: Option<&Vec<RawAllowPermission>>,
) -> Result<Option<AllowRule>, PolicyError> {
    let Some(entries) = entries else {
        return Ok(None);
    };
    // A method can carry more than one `allow_permission`; Django runs them in
    // wrapper order (the outermost decorator first). The inventory records them
    // in `wrapper_chain` order, which is that order. Only one route today has
    // more than one, and only on plain views.
    let mut rules = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.unknown {
            return Err(PolicyError::UnresolvedAllowPermission {
                order,
                path: path.to_string(),
                method: method.to_string(),
                what: "unreadable arguments".to_string(),
            });
        }
        let Some(roles) = &entry.allowed_roles else {
            return Err(PolicyError::UnresolvedAllowPermission {
                order,
                path: path.to_string(),
                method: method.to_string(),
                what: "no `allowed_roles`".to_string(),
            });
        };
        let mut parsed: Vec<Role> = Vec::with_capacity(roles.len());
        for role in roles {
            let Some(role) = Role::from_name(role) else {
                return Err(PolicyError::UnresolvedAllowPermission {
                    order,
                    path: path.to_string(),
                    method: method.to_string(),
                    what: format!("unknown role {role:?}"),
                });
            };
            parsed.push(role);
        }
        // `AllowPermission` carries a `&'static [Role]` so the policy can be a
        // `static`; the table is loaded once at startup, so leaking this tiny
        // vector is deliberate and bounded (see `Registry::new`).
        let roles: &'static [Role] = Box::leak(parsed.into_boxed_slice());
        let level = Level::parse(entry.level.as_deref().unwrap_or("PROJECT"));
        let model = match &entry.model {
            Some(model) => Some(CreatorModel::from_dotted(model).ok_or_else(|| {
                PolicyError::UnresolvedAllowPermission {
                    order,
                    path: path.to_string(),
                    method: method.to_string(),
                    what: format!("unknown creator model {model:?}"),
                }
            })?),
            None => None,
        };
        rules.push(AllowRule {
            permission: AllowPermission::new(roles, level, entry.creator, model),
        });
    }
    // The runtime supports one rule; a second is a policy error rather than a
    // silently dropped check.
    match rules.len() {
        0 => Ok(None),
        1 => Ok(rules.pop()),
        _ => Err(PolicyError::UnresolvedAllowPermission {
            order,
            path: path.to_string(),
            method: method.to_string(),
            what: "more than one allow_permission decorator".to_string(),
        }),
    }
}

/// Every lifecycle override a concrete view declares must have an
/// implementation in this crate (or in the family base, for the *api*
/// families whose `get_throttles` is part of the base class).
fn validate_lifecycle(
    raw: &RawRoute,
    family: Family,
    dynamic_throttles: bool,
) -> Result<(), PolicyError> {
    let error = |hook: &'static str| {
        Err(PolicyError::UnhandledLifecycleOverride {
            view: raw.view.clone(),
            hook,
            owner: raw
                .lifecycle_overrides
                .get(hook)
                .and_then(|owner| owner.clone())
                .unwrap_or_default(),
        })
    };
    for hook in [
        "dispatch",
        "handle_exception",
        "finalize_response",
        "options",
        "get_authenticators",
    ] {
        if raw
            .lifecycle_overrides
            .get(hook)
            .and_then(|owner| owner.as_ref())
            .is_some()
        {
            return error(hook);
        }
    }
    if let Some(owner) = raw
        .lifecycle_overrides
        .get("initial")
        .and_then(|owner| owner.as_ref())
        && !initial_issue_hidden_from_guest(&raw.view)
    {
        let _ = owner;
        return error("initial");
    }
    if let Some(owner) = raw
        .lifecycle_overrides
        .get("get_throttles")
        .and_then(|owner| owner.as_ref())
    {
        let family_owned = matches!(family, Family::ApiApiView | Family::ApiViewSet)
            && owner == "plane.api.views.base.BaseAPIView";
        if !family_owned
            && !(dynamic_throttles && raw.view == "plane.app.views.user.base.UserEndpoint")
        {
            return error("get_throttles");
        }
    }
    if let Some(owner) = raw
        .lifecycle_overrides
        .get("get_permissions")
        .and_then(|owner| owner.as_ref())
    {
        let _ = owner;
        if !raw.dynamic_permissions {
            return error("get_permissions");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> PolicyTable {
        PolicyTable::load().expect("routes.json")
    }

    /// `[m.upper() for m in http_method_names if hasattr(self, m)]` — in
    /// `http_method_names` order, never sorted, and including a method the
    /// `BaseViewSet` guard refuses (it is still bound).
    #[test]
    fn allow_header_order_matches_django() {
        let table = table();
        let cycle = table
            .routes()
            .iter()
            .find(|route| {
                route.path_template == "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/"
            })
            .expect("cycle detail route");
        // GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy.
        assert_eq!(
            cycle.allow.as_deref(),
            Some("GET, PUT, PATCH, DELETE, HEAD, OPTIONS")
        );
        // The PUT handler is registered but refused at runtime, and is still in
        // `Allow`.
        let put = cycle.method(&Method::PUT).expect("put");
        assert!(put.refused_by_guard);
        assert!(cycle.allow.as_deref().unwrap().contains("PUT"));

        let analytics = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("analytics");
        assert_eq!(analytics.allow.as_deref(), Some("GET, HEAD, OPTIONS"));

        // Function views never restrict methods.
        let health = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.web.views.health_check")
            .expect("health");
        assert_eq!(health.allow, None);
        assert_eq!(health.family, Family::Function);
    }

    /// Every family's `handle_exception` wording (§11.2).
    #[test]
    fn family_exception_mapping() {
        assert_eq!(
            Family::AppViewSet.exception(ViewError::Integrity),
            ApiError::InvalidPayload
        );
        assert_eq!(
            Family::AppViewSet.exception(ViewError::Validation),
            ApiError::InvalidDetail
        );
        assert_eq!(
            Family::AppViewSet.exception(ViewError::ObjectDoesNotExist),
            ApiError::ObjectDoesNotExist
        );
        assert_eq!(
            Family::AppViewSet.exception(ViewError::Key),
            ApiError::MissingKey
        );
        assert_eq!(
            Family::AppViewSet.exception(ViewError::Unknown("boom".into())),
            ApiError::FamilyServerError
        );
        // `/api/v1/`'s BaseAPIView is the only "requested resource" family.
        assert_eq!(
            Family::ApiApiView.exception(ViewError::ObjectDoesNotExist),
            ApiError::ResourceDoesNotExist
        );
        assert_eq!(
            Family::ApiViewSet.exception(ViewError::ObjectDoesNotExist),
            ApiError::ObjectDoesNotExist
        );
        assert_eq!(
            Family::SpaceViewSet.exception(ViewError::ObjectDoesNotExist),
            ApiError::ObjectDoesNotExist
        );
        assert_eq!(
            Family::LicenseApiView.exception(ViewError::ObjectDoesNotExist),
            ApiError::ObjectDoesNotExist
        );
        // A plain DRF view re-raises everything that is not an APIException.
        assert_eq!(
            Family::DrfApiView.exception(ViewError::Integrity),
            ApiError::InternalServerError
        );
        assert_eq!(
            Family::DjangoView.exception(ViewError::Unknown("x".into())),
            ApiError::InternalServerError
        );
        // An `APIException` is already mapped.
        assert_eq!(
            Family::AppViewSet.exception(ViewError::Api(ApiError::NotAcceptable)),
            ApiError::NotAcceptable
        );
    }

    #[test]
    fn read_replica_flags_are_per_route() {
        let table = table();
        let user = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.user.base.UserEndpoint")
            .expect("user endpoint");
        assert!(user.use_read_replica, "UserEndpoint opts in");
        let analytics = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("analytics");
        assert!(!analytics.use_read_replica);
        assert_eq!(
            table
                .routes()
                .iter()
                .filter(|route| route.use_read_replica)
                .count(),
            91
        );
    }

    #[test]
    fn families_are_the_inventory_families() {
        let table = table();
        for route in table.routes() {
            assert!(
                route.view_name.is_some() || !route.family.is_drf(),
                "{} has no view name",
                route.view
            );
            if route.family.is_drf() {
                // Every renderer class must be mapped for negotiation; only the
                // three drf-spectacular routes (group `schema`) differ from
                // Plane's single `JSONRenderer`.
                let specs = crate::negotiate::renderer_specs(&route.renderer_classes)
                    .unwrap_or_else(|| panic!("{} has an unmapped renderer class", route.view));
                assert!(!specs.is_empty(), "{} has no renderer", route.view);
                if route.group != "schema" {
                    assert_eq!(specs, vec![crate::negotiate::JSON_RENDERER]);
                }
            }
        }
    }

    /// `BaseViewSet.initial` refuses a mixin-served action only when the
    /// effective permissions authorize nothing (§ hazard (g)).
    #[test]
    fn guard_rule_uses_the_effective_permissions() {
        let table = table();
        let route = table
            .routes()
            .iter()
            .find(|route| {
                route.view == "plane.app.views.cycle.issue.CycleIssueViewSet"
                    && route
                        .guard_actions
                        .get("retrieve")
                        .is_some_and(|action| action.mixin_provided)
            })
            .expect("cycle issue retrieve route");
        let action = route.guard_actions.get("retrieve").expect("retrieve");
        assert!(action.mixin_provided);
        assert!(!action.project_implemented);
        // `routes.json` marks the method `[405]` for the static case.
        assert!(
            route
                .methods
                .iter()
                .any(|entry| entry.action.as_deref() == Some("retrieve") && entry.refused_by_guard)
        );
    }

    #[test]
    fn lifecycle_overrides_are_resolved() {
        let table = table();
        let subscriber = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.issue.subscriber.IssueSubscriberViewSet")
            .expect("subscriber");
        assert!(subscriber.initial_issue_hidden_from_guest);
        assert!(subscriber.dynamic_permissions);
        let user = table
            .routes()
            .iter()
            .find(|route| route.view == "plane.app.views.user.base.UserEndpoint")
            .expect("user endpoint");
        assert!(user.user_endpoint_throttles);
        assert_eq!(
            user.effective_throttles(Some("update_email")),
            &[ThrottleClass::Authentication]
        );
        assert_eq!(
            user.effective_throttles(Some("generate_email_verification_code")),
            &[
                ThrottleClass::EmailVerification,
                ThrottleClass::Authentication
            ]
        );
        assert_eq!(
            user.effective_throttles(Some("retrieve")),
            &[ThrottleClass::Anon]
        );
        // The v1 families: `BaseAPIView.get_throttles` replaces the defaults
        // with the API-key throttle; `BaseViewSet` does not, so its rows keep
        // `AnonRateThrottle` (`plane/api/views/base.py:69-70,162-279`).
        let v1 = table
            .routes()
            .iter()
            .find(|route| route.group == "v1" && route.family == Family::ApiApiView)
            .expect("v1 api view");
        assert_eq!(v1.throttles, vec![ThrottleClass::ApiKey]);
        let v1_viewset = table
            .routes()
            .iter()
            .find(|route| route.group == "v1" && route.family == Family::ApiViewSet)
            .expect("v1 viewset");
        assert_eq!(
            v1_viewset.throttles,
            vec![ThrottleClass::Anon],
            "BaseViewSet inherits DEFAULT_THROTTLE_CLASSES, it does not override get_throttles"
        );
        // The two `drf.APIView` api-roots are not part of a Plane family
        // either: they keep the default `AnonRateThrottle`.
        let v1_root = table
            .routes()
            .iter()
            .find(|route| route.group == "v1" && route.family == Family::DrfApiView)
            .expect("v1 api root");
        assert_eq!(v1_root.throttles, vec![ThrottleClass::Anon]);
    }
}
