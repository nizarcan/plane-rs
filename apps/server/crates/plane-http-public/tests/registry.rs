// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Invariants of `register_all` for the `/api/v1/` surface.
//!
//! Same shape as `plane-http-app/tests/registry.rs`, narrowed to what a v1 set
//! can break:
//!
//! 1. `register_all` succeeds on a fresh [`Registry`] built from the embedded
//!    policy. Registration is stateful and one place owns the sequence, so a
//!    duplicated or stale registration is a startup failure and must stay one.
//! 2. Every registered `(view, action or method)` resolves to a route whose
//!    `routes.json` `group` is `v1`. This crate serves `/api/v1/`; a handler
//!    for another group would be dead code in the wrong module.
//!
//! `ROUTES.md`'s Owner column is deliberately not checked here: a set
//! registers a route before the orchestrator flips the proxy, and the proxy
//! checks that direction from `plane-http-app`'s side of the surface.

use http::Method;
use plane_drf::policy::PolicyTable;
use plane_drf::registry::Registry;

#[test]
fn register_all_succeeds_and_only_serves_v1() {
    let policy = PolicyTable::load().expect("embedded policy loads");
    let mut registry = Registry::new(policy).expect("registry");
    plane_http_public::register_all(&mut registry).expect("register_all");

    let policy = registry.policy();
    let mut registered = 0;
    for (view, entry) in registry.views() {
        for name in entry.registered() {
            let routes: Vec<_> = policy
                .routes()
                .iter()
                .filter(|route| route.view == *view)
                .collect();
            assert!(!routes.is_empty(), "{view} has no route");
            for route in routes {
                assert_eq!(
                    route.group, "v1",
                    "{view} {name} is registered by plane-http-public but its route \
                     (order {}) belongs to group {:?}",
                    route.order, route.group
                );
            }
            registered += 1;
        }
    }
    assert!(registered > 0, "register_all registered nothing");
    // Every method a v1 view registers must be one `routes.json` maps for it.
    for (view, entry) in registry.views() {
        for method in [Method::GET, Method::POST, Method::PATCH, Method::DELETE] {
            if entry.has_method(&method) {
                assert!(
                    entry.handler(&route_for(policy, view), &method).is_some(),
                    "{view} {method} does not resolve"
                );
            }
        }
    }
}

fn route_for(policy: &PolicyTable, view: &str) -> plane_drf::policy::RoutePolicy {
    policy
        .routes()
        .iter()
        .find(|route| route.view == view)
        .cloned()
        .expect("view has a route")
}
