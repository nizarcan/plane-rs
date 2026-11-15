// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `policy-probe` cargo feature.
//!
//! Compiled only with `--features policy-probe`, this registers a handler for
//! **every** route method `routes.json` lists outside `/api/v1/`, so the
//! differential harness can drive each route through the full runtime policy
//! (content negotiation, authentication, permissions, throttles, the family
//! `initial` hooks, `allow_permission`) and observe the outcome.
//!
//! The probe handler returns
//!
//! ```text
//! 501 Not Implemented
//! X-Plane-RS-Probe: handler
//! ```
//!
//! and is reached only after the runtime's policy passed. The harness treats a
//! step whose B response carries that header as a pass-through: if A (Django)
//! denied the request (401/403/405/406/415/429, or the `allow_permission` 403
//! body) the step is a DIFF — Rust let through what Django refused; otherwise
//! the step is recorded as `reached-handler` and its response and side effects
//! are not compared.
//!
//! `/api/v1/` is excluded on purpose: `APITokenLogMiddleware` would have to
//! publish a Celery task, the publisher is Phase 4's, and the registry refuses
//! those views (see `plane_drf::token_log`). The probe counts therefore cover
//! 557 method pairs, not the 708 in the inventory.

use http::Method;
use plane_drf::error::ViewError;
use plane_drf::policy::Family;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

/// The header and value the harness recognises.
pub const PROBE_HEADER: &str = "X-Plane-RS-Probe";
pub const PROBE_VALUE: &str = "handler";

fn probe<'a>(_ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { Ok(ViewResponse::empty(501).with_header(PROBE_HEADER, PROBE_VALUE)) })
}

/// Register the probe for every non-v1 route method.
pub fn register_all(registry: &mut Registry) -> Result<(), RegistryError> {
    // Collect first: registering borrows the registry mutably.
    let mut kinds = std::collections::BTreeMap::new();
    let mut actions: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut methods: std::collections::BTreeMap<String, Vec<Method>> =
        std::collections::BTreeMap::new();
    for route in registry
        .policy()
        .routes()
        .iter()
        .filter(|route| route.group != "v1" && route.group != "web")
    {
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

    for (view, family) in &kinds {
        if *family == Family::Function {
            registry.function(view)?.all(probe)?;
        } else if family.is_viewset() {
            let mut builder = registry.viewset(view)?;
            for action in actions.get(view).cloned().unwrap_or_default() {
                builder.action(&action, probe)?;
            }
        } else if family.is_drf() {
            let mut builder = registry.api_view(view)?;
            for method in methods.get(view).cloned().unwrap_or_default() {
                builder.method(method, probe)?;
            }
        } else {
            let mut builder = registry.django_view(view)?;
            for method in methods.get(view).cloned().unwrap_or_default() {
                builder.method(method, probe)?;
            }
        }
    }
    Ok(())
}

/// A startup check used by the probe test: every non-v1 method pair resolves.
#[cfg(test)]
pub fn registered_pairs(registry: &Registry) -> anyhow::Result<usize> {
    let mut pairs = 0;
    for route in registry
        .policy()
        .routes()
        .iter()
        .filter(|route| route.group != "v1" && route.group != "web")
    {
        let Some(view) = registry.view(&route.view) else {
            anyhow::bail!("{} has a policy but no registration", route.view);
        };
        for entry in &route.methods {
            if view.handler(route, &entry.method).is_none() {
                anyhow::bail!(
                    "{}.{:?} has a policy but no probe handler",
                    route.view,
                    entry.action
                );
            }
            pairs += 1;
        }
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_drf::policy::PolicyTable;

    #[test]
    fn the_probe_covers_every_non_v1_method_pair() {
        let policy = PolicyTable::load().expect("routes.json");
        let mut registry = Registry::new(policy).expect("registry");
        register_all(&mut registry).expect("register");
        let registry = registry.finish().expect("finish");
        assert_eq!(registered_pairs(&registry).expect("pairs"), 541);
        // `/api/v1/` is the one surface the Celery publisher blocks.
        assert!(
            registry
                .view("plane.api.views.cycle.CycleListCreateAPIEndpoint")
                .is_none()
        );
        // The web function views are registered by `startup`, not the probe.
        assert!(registry.view("plane.web.views.health_check").is_none());
        assert!(
            registry
                .view("plane.authentication.views.app.signout.SignOutAuthEndpoint")
                .is_some()
        );
    }

    #[test]
    fn the_probe_header_is_the_harnesss_marker() {
        assert_eq!(PROBE_HEADER, "X-Plane-RS-Probe");
        assert_eq!(PROBE_VALUE, "handler");
    }
}
