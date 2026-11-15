// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The handler registry: the only way a route becomes reachable.
//!
//! A handler is registered by **Django view dotted path** plus **ViewSet
//! action** (for a ViewSet) or **HTTP method** (for an APIView, a plain
//! `django.views.View` or a function view). Paths, converters, methods,
//! `Allow` ordering and every security class come from `routes.json`; nothing
//! about a route can be declared here.
//!
//! ```ignore
//! use plane_drf::registry::Registry;
//!
//! let policy = plane_drf::policy::PolicyTable::load()?;
//! let mut registry = Registry::new(policy)?;
//! let states = registry.viewset("plane.app.views.state.base.StateViewSet")?;
//! states.action("list", handlers::states::list)?;
//! states.action("create", handlers::states::create)?;
//! let registered = registry.finish()?;
//! ```
//!
//! Registration fails when:
//!
//! * the dotted path is not in `routes.json`;
//! * the view is registered through the wrong builder (a ViewSet through
//!   `api_view`, and so on);
//! * the action or method is not one Django maps to that view;
//! * the same action or method is registered twice;
//! * the route is under `/api/v1/`, whose `APITokenLogMiddleware` side effect
//!   needs the Celery publisher that Phase 4 adds (see [`RegistryError`]);
//! * the view's policy has an override that `plane-drf` does not implement —
//!   `PolicyTable::load` refuses those at startup, so this cannot happen for a
//!   loaded policy, but the builder still checks.
//!
//! An unregistered route keeps answering the pipeline's 501 (Phase 1's
//! behaviour); the registry never invents a handler for it.

use std::collections::BTreeMap;

use http::Method;

use crate::policy::{Family, PolicyTable, RoutePolicy};
use crate::view::HandlerFn;

/// Why a registration was refused.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("no route in routes.json has view {view:?}")]
    UnknownView { view: String },
    #[error("{view} is a {family:?}, not a ViewSet")]
    NotAViewSet { view: String, family: Family },
    #[error("{view} is a {family:?}, not a plain APIView")]
    NotAnApiView { view: String, family: Family },
    #[error("{view} is a {family:?}, not a django.views.View")]
    NotADjangoView { view: String, family: Family },
    #[error("{view} is a {family:?}, not a function view")]
    NotAFunction { view: String, family: Family },
    #[error("{view}: routes.json has no action {action:?} (it has: {known})")]
    UnknownAction {
        view: String,
        action: String,
        known: String,
    },
    #[error("{view}: routes.json registers no {method} method (it has: {known})")]
    UnknownMethod {
        view: String,
        method: String,
        known: String,
    },
    #[error("{view}: action {action:?} is registered twice")]
    DuplicateAction { view: String, action: String },
    #[error("{view}: method {method} is registered twice")]
    DuplicateMethod { view: String, method: String },
    #[error(
        "{view}: /api/v1/ routes need APITokenLogMiddleware, which enqueues the Celery task \
         `plane.bgtasks.logger_task.process_logs`; the Celery publisher arrives in Phase 4, so \
         registering this route now would silently drop the api_activity_logs row"
    )]
    ApiTokenLogUnavailable { view: String },
    #[error("{view}: the policy needs manual handling ({reason}); no override is implemented")]
    UnhandledPolicy { view: String, reason: String },
}

/// One registered view.
#[derive(Debug)]
pub struct RegisteredView {
    pub view: String,
    pub family: Family,
    /// ViewSet actions.
    actions: BTreeMap<String, HandlerFn>,
    /// APIView / plain view / function handlers by HTTP method.
    methods: BTreeMap<Method, HandlerFn>,
    /// The first route of this view, used for class-level facts.
    pub route_order: u32,
}

impl RegisteredView {
    /// The handler for a request.
    ///
    /// A ViewSet is looked up by action (`ViewSetMixin.initialize_request`);
    /// everything else by HTTP method, with `HEAD` falling back to `GET`
    /// (Django's `View.setup`).
    pub fn handler(&self, route: &RoutePolicy, method: &Method) -> Option<HandlerFn> {
        if self.family.is_viewset() {
            let action = route.action_for(method)?;
            return self.actions.get(action).copied();
        }
        if let Some(handler) = self.methods.get(method) {
            return Some(*handler);
        }
        if *method == Method::HEAD {
            return self.methods.get(&Method::GET).copied();
        }
        None
    }

    /// Whether the view answers OPTIONS with `APIView.options` metadata.
    pub fn options_is_metadata(&self) -> bool {
        self.family.is_drf() && !self.family.is_viewset() || self.family.is_viewset()
    }

    pub fn is_function(&self) -> bool {
        self.family == Family::Function
    }

    /// Whether this view registered a handler for the method (no `HEAD`
    /// fallback).
    pub fn has_method(&self, method: &Method) -> bool {
        self.methods.contains_key(method)
    }

    /// Every registered `(action or method, handler)` pair, for the coverage
    /// test that proves a handler cannot exist without a policy.
    pub fn registered(&self) -> Vec<String> {
        let mut names: Vec<String> = self.actions.keys().cloned().collect();
        let mut methods: Vec<String> = self
            .methods
            .keys()
            .map(|method| method.as_str().to_uppercase())
            .collect();
        names.append(&mut methods);
        names
    }
}

/// The registry of every route the binary serves itself.
#[derive(Debug, Default)]
pub struct Registry {
    policy: Option<PolicyTable>,
    views: BTreeMap<String, RegisteredView>,
}

impl Registry {
    pub fn new(policy: PolicyTable) -> Result<Self, RegistryError> {
        Ok(Self {
            policy: Some(policy),
            views: BTreeMap::new(),
        })
    }

    pub fn policy(&self) -> &PolicyTable {
        self.policy.as_ref().expect("policy is set in new()")
    }

    /// The registered views, for dispatch and tests.
    pub fn views(&self) -> &BTreeMap<String, RegisteredView> {
        &self.views
    }

    pub fn view(&self, view: &str) -> Option<&RegisteredView> {
        self.views.get(view)
    }

    /// Begin registering a ViewSet.
    pub fn viewset<'a>(&'a mut self, view: &'a str) -> Result<ViewsetBuilder<'a>, RegistryError> {
        let family = self.family_of(view)?;
        if !family.is_viewset() {
            return Err(RegistryError::NotAViewSet {
                view: view.to_string(),
                family,
            });
        }
        self.ensure_v1_allowed(view)?;
        self.ensure_policy_handled(view)?;
        let route_order = self
            .policy()
            .routes()
            .iter()
            .find(|route| route.view == view)
            .map(|route| route.order)
            .unwrap_or_default();
        self.views
            .entry(view.to_string())
            .or_insert(RegisteredView {
                view: view.to_string(),
                family,
                actions: BTreeMap::new(),
                methods: BTreeMap::new(),
                route_order,
            });
        Ok(ViewsetBuilder {
            registry: self,
            view,
        })
    }

    /// Begin registering a DRF `APIView` / `GenericAPIView`.
    pub fn api_view<'a>(&'a mut self, view: &'a str) -> Result<ApiViewBuilder<'a>, RegistryError> {
        let family = self.family_of(view)?;
        if !family.is_drf() || family.is_viewset() {
            return Err(RegistryError::NotAnApiView {
                view: view.to_string(),
                family,
            });
        }
        self.ensure_v1_allowed(view)?;
        self.ensure_policy_handled(view)?;
        let route_order = self.first_route_order(view);
        self.views
            .entry(view.to_string())
            .or_insert(RegisteredView {
                view: view.to_string(),
                family,
                actions: BTreeMap::new(),
                methods: BTreeMap::new(),
                route_order,
            });
        Ok(ApiViewBuilder {
            registry: self,
            view,
        })
    }

    /// Begin registering a plain `django.views.View` subclass.
    pub fn django_view<'a>(
        &'a mut self,
        view: &'a str,
    ) -> Result<PlainViewBuilder<'a>, RegistryError> {
        let family = self.family_of(view)?;
        if family != Family::DjangoView {
            return Err(RegistryError::NotADjangoView {
                view: view.to_string(),
                family,
            });
        }
        self.ensure_v1_allowed(view)?;
        self.ensure_policy_handled(view)?;
        let route_order = self.first_route_order(view);
        self.views
            .entry(view.to_string())
            .or_insert(RegisteredView {
                view: view.to_string(),
                family,
                actions: BTreeMap::new(),
                methods: BTreeMap::new(),
                route_order,
            });
        Ok(PlainViewBuilder {
            registry: self,
            view,
        })
    }

    /// Begin registering a function view.
    pub fn function<'a>(&'a mut self, view: &'a str) -> Result<FunctionBuilder<'a>, RegistryError> {
        let family = self.family_of(view)?;
        if family != Family::Function {
            return Err(RegistryError::NotAFunction {
                view: view.to_string(),
                family,
            });
        }
        self.ensure_v1_allowed(view)?;
        self.ensure_policy_handled(view)?;
        let route_order = self.first_route_order(view);
        self.views
            .entry(view.to_string())
            .or_insert(RegisteredView {
                view: view.to_string(),
                family,
                actions: BTreeMap::new(),
                methods: BTreeMap::new(),
                route_order,
            });
        Ok(FunctionBuilder {
            registry: self,
            view,
        })
    }

    /// Finish registration.
    ///
    /// Nothing has to be complete: an unregistered route still 501s. The check
    /// that does exist is per registered view: it must keep at least one
    /// handler, and every registered action/method must resolve to a policy
    /// entry (enforced incrementally, re-asserted here).
    pub fn finish(self) -> Result<Self, RegistryError> {
        for (view, registered) in &self.views {
            if registered.actions.is_empty() && registered.methods.is_empty() {
                return Err(RegistryError::UnknownView { view: view.clone() });
            }
        }
        Ok(self)
    }

    fn family_of(&self, view: &str) -> Result<Family, RegistryError> {
        self.policy()
            .views()
            .get(view)
            .map(|policy| policy.family)
            .ok_or_else(|| RegistryError::UnknownView {
                view: view.to_string(),
            })
    }

    fn first_route_order(&self, view: &str) -> u32 {
        self.policy()
            .routes()
            .iter()
            .find(|route| route.view == view)
            .map(|route| route.order)
            .unwrap_or_default()
    }

    fn routes_for<'a>(&'a self, view: &'a str) -> impl Iterator<Item = &'a RoutePolicy> + 'a {
        self.policy()
            .routes()
            .iter()
            .filter(move |route| route.view == view)
    }

    /// `/api/v1/` is blocked until Phase 4's Celery publisher exists.
    fn ensure_v1_allowed(&self, view: &str) -> Result<(), RegistryError> {
        if self.routes_for(view).any(|route| route.group == "v1") {
            return Err(RegistryError::ApiTokenLogUnavailable {
                view: view.to_string(),
            });
        }
        Ok(())
    }

    /// A view whose policy needs an override this crate does not have must
    /// never be registered.
    fn ensure_policy_handled(&self, view: &str) -> Result<(), RegistryError> {
        for route in self.routes_for(view) {
            if route.dynamic_authenticators {
                return Err(RegistryError::UnhandledPolicy {
                    view: view.to_string(),
                    reason: "dynamic_authenticators".to_string(),
                });
            }
            if route.dynamic_permissions
                && !plane_permissions::dynamic::DYNAMIC_VIEWS.contains(&view)
                && !plane_permissions::dynamic::SUBCLASS_ALIASES
                    .iter()
                    .any(|(alias, _)| *alias == view)
            {
                return Err(RegistryError::UnhandledPolicy {
                    view: view.to_string(),
                    reason: "get_permissions override with no implementation".to_string(),
                });
            }
            if route.dynamic_throttles && route.view != "plane.app.views.user.base.UserEndpoint" {
                return Err(RegistryError::UnhandledPolicy {
                    view: view.to_string(),
                    reason: "get_throttles override with no implementation".to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Builder for a ViewSet's actions.
pub struct ViewsetBuilder<'r> {
    registry: &'r mut Registry,
    view: &'r str,
}

impl ViewsetBuilder<'_> {
    /// Register one ViewSet action (`list`, `create`, `retrieve`, …).
    pub fn action(&mut self, action: &str, handler: HandlerFn) -> Result<&mut Self, RegistryError> {
        let policy = self.registry.policy();
        let known: Vec<&str> = policy
            .routes()
            .iter()
            .filter(|route| route.view == self.view)
            .flat_map(|route| route.methods.iter())
            .filter_map(|entry| entry.action.as_deref())
            .collect();
        if !known.contains(&action) {
            return Err(RegistryError::UnknownAction {
                view: self.view.to_string(),
                action: action.to_string(),
                known: joined(&known),
            });
        }
        let registered =
            self.registry
                .views
                .get_mut(self.view)
                .ok_or_else(|| RegistryError::UnknownView {
                    view: self.view.to_string(),
                })?;
        if registered
            .actions
            .insert(action.to_string(), handler)
            .is_some()
        {
            return Err(RegistryError::DuplicateAction {
                view: self.view.to_string(),
                action: action.to_string(),
            });
        }
        Ok(self)
    }
}

/// Builder for an APIView's methods.
pub struct ApiViewBuilder<'r> {
    registry: &'r mut Registry,
    view: &'r str,
}

impl ApiViewBuilder<'_> {
    pub fn method(
        &mut self,
        method: Method,
        handler: HandlerFn,
    ) -> Result<&mut Self, RegistryError> {
        self.register(method, handler)
    }

    fn register(&mut self, method: Method, handler: HandlerFn) -> Result<&mut Self, RegistryError> {
        register_method(self.registry, self.view, method, handler)?;
        Ok(self)
    }
}

/// Builder for a plain `django.views.View`.
pub struct PlainViewBuilder<'r> {
    registry: &'r mut Registry,
    view: &'r str,
}

impl PlainViewBuilder<'_> {
    pub fn method(
        &mut self,
        method: Method,
        handler: HandlerFn,
    ) -> Result<&mut Self, RegistryError> {
        register_method(self.registry, self.view, method, handler)?;
        Ok(self)
    }
}

/// Builder for a function view.
pub struct FunctionBuilder<'r> {
    registry: &'r mut Registry,
    view: &'r str,
}

impl FunctionBuilder<'_> {
    pub fn method(
        &mut self,
        method: Method,
        handler: HandlerFn,
    ) -> Result<&mut Self, RegistryError> {
        register_method(self.registry, self.view, method, handler)?;
        Ok(self)
    }

    /// Register every method `routes.json` lists for the function.
    pub fn all(&mut self, handler: HandlerFn) -> Result<&mut Self, RegistryError> {
        let methods: Vec<Method> = self
            .registry
            .routes_for(self.view)
            .flat_map(|route| route.methods.iter().map(|entry| entry.method.clone()))
            .collect();
        for method in methods {
            register_method(self.registry, self.view, method, handler)?;
        }
        Ok(self)
    }
}

fn register_method(
    registry: &mut Registry,
    view: &str,
    method: Method,
    handler: HandlerFn,
) -> Result<(), RegistryError> {
    let policy = registry.policy();
    let has_method = policy
        .routes()
        .iter()
        .filter(|route| route.view == view)
        .any(|route| route.methods.iter().any(|entry| entry.method == method));
    if !has_method {
        let known: Vec<String> = policy
            .routes()
            .iter()
            .filter(|route| route.view == view)
            .flat_map(|route| route.methods.iter())
            .map(|entry| entry.method.as_str().to_uppercase())
            .collect();
        return Err(RegistryError::UnknownMethod {
            view: view.to_string(),
            method: method.as_str().to_uppercase(),
            known: joined(&known.iter().map(String::as_str).collect::<Vec<_>>()),
        });
    }
    let registered = registry
        .views
        .get_mut(view)
        .ok_or_else(|| RegistryError::UnknownView {
            view: view.to_string(),
        })?;
    if registered.methods.insert(method.clone(), handler).is_some() {
        return Err(RegistryError::DuplicateMethod {
            view: view.to_string(),
            method: method.as_str().to_uppercase(),
        });
    }
    Ok(())
}

fn joined(values: &[&str]) -> String {
    let mut unique: Vec<&str> = values.to_vec();
    unique.sort_unstable();
    unique.dedup();
    unique.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{BoxFuture, ViewCtx, ViewResponse};

    fn policy() -> PolicyTable {
        PolicyTable::load().expect("routes.json")
    }

    fn probe<'a>(
        _ctx: &'a ViewCtx<'a>,
    ) -> BoxFuture<'a, Result<ViewResponse, crate::error::ViewError>> {
        Box::pin(async move { Ok(ViewResponse::empty(501)) })
    }

    #[test]
    fn a_viewset_registers_by_action() {
        let mut registry = Registry::new(policy()).expect("registry");
        registry
            .viewset("plane.app.views.state.base.StateViewSet")
            .expect("viewset")
            .action("list", probe)
            .expect("list")
            .action("create", probe)
            .expect("create");
        let registry = registry.finish().expect("finish");
        let view = registry
            .view("plane.app.views.state.base.StateViewSet")
            .expect("registered");
        assert!(view.actions.contains_key("list"));
        assert!(view.actions.contains_key("create"));
    }

    #[test]
    fn unknown_names_are_refused() {
        let mut registry = Registry::new(policy()).expect("registry");
        assert!(matches!(
            registry.viewset("plane.app.views.not.a.view"),
            Err(RegistryError::UnknownView { .. })
        ));
        let mut builder = registry
            .viewset("plane.app.views.state.base.StateViewSet")
            .expect("viewset");
        assert!(matches!(
            builder.action("nope", probe),
            Err(RegistryError::UnknownAction { .. })
        ));
        // A ViewSet cannot be registered as an APIView.
        assert!(matches!(
            registry.api_view("plane.app.views.state.base.StateViewSet"),
            Err(RegistryError::NotAnApiView { .. })
        ));
    }

    #[test]
    fn duplicates_are_refused() {
        let mut registry = Registry::new(policy()).expect("registry");
        let mut builder = registry
            .api_view("plane.app.views.analytic.base.AnalyticsEndpoint")
            .expect("api view");
        builder.method(Method::GET, probe).expect("get");
        assert!(matches!(
            builder.method(Method::GET, probe),
            Err(RegistryError::DuplicateMethod { .. })
        ));
    }

    #[test]
    fn v1_is_blocked_until_the_celery_publisher_exists() {
        let mut registry = Registry::new(policy()).expect("registry");
        assert!(matches!(
            registry.api_view("plane.api.views.cycle.CycleListCreateAPIEndpoint"),
            Err(RegistryError::ApiTokenLogUnavailable { .. })
        ));
    }

    #[test]
    fn every_registered_handler_has_a_policy() {
        // The brief's coverage test: register a probe handler for every
        // non-v1 route method through the public API, then prove each
        // registration resolves to a policy method entry.
        let policy = policy();
        let mut kinds: BTreeMap<String, Family> = BTreeMap::new();
        let mut actions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut methods: BTreeMap<String, Vec<Method>> = BTreeMap::new();
        for route in policy.routes().iter().filter(|route| route.group != "v1") {
            kinds.insert(route.view.clone(), route.family);
            for entry in &route.methods {
                if route.family.is_viewset() {
                    let list = actions.entry(route.view.clone()).or_default();
                    let action = entry.action.clone().unwrap_or_default();
                    if !list.contains(&action) {
                        list.push(action);
                    }
                } else {
                    let list = methods.entry(route.view.clone()).or_default();
                    if !list.contains(&entry.method) {
                        list.push(entry.method.clone());
                    }
                }
            }
        }

        let mut registry = Registry::new(policy).expect("registry");
        for (view, family) in &kinds {
            if *family == Family::Function {
                let mut builder = registry.function(view).expect("function");
                builder.all(probe).expect("all methods");
            } else if family.is_viewset() {
                let mut builder = registry.viewset(view).expect("viewset");
                for action in actions.get(view).cloned().unwrap_or_default() {
                    builder.action(&action, probe).expect("action");
                }
            } else if family.is_drf() {
                let mut builder = registry.api_view(view).expect("api view");
                for method in methods.get(view).cloned().unwrap_or_default() {
                    builder.method(method, probe).expect("method");
                }
            } else {
                let mut builder = registry.django_view(view).expect("plain view");
                for method in methods.get(view).cloned().unwrap_or_default() {
                    builder.method(method, probe).expect("method");
                }
            }
        }
        let registry = registry.finish().expect("finish");

        // Every route method of every registered view has a handler, and every
        // handler resolves through the same policy route the pipeline will use.
        let mut checked = 0;
        for route in registry
            .policy()
            .routes()
            .iter()
            .filter(|route| route.group != "v1")
        {
            let registered = registry.view(&route.view).expect("registered view");
            for entry in &route.methods {
                assert!(
                    registered.handler(route, &entry.method).is_some(),
                    "{}.{:?} has a policy but no handler",
                    route.view,
                    entry.action
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 557, "non-v1 registered method pairs");
    }
}
