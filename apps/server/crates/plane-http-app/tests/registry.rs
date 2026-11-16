// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! What `register_all` actually registers.
//!
//! The assertion is deliberately exact. A route that becomes reachable on Rust
//! without its owner column in `ROUTES.md` being flipped is a coexistence bug
//! (the proxy would still send that path to Django, so the port would be
//! invisible and untested), and a group that starts answering from Rust while
//! its scenario is not clean is worse. So the expected list below changes in the
//! same commit as `ROUTES.md` and the scenario, never on its own.

use plane_drf::policy::PolicyTable;
use plane_drf::registry::Registry;

#[test]
fn register_all_registers_the_worked_example_and_nothing_else() {
    let policy = PolicyTable::load().expect("routes.json");
    let mut registry = Registry::new(policy).expect("registry");
    plane_http_app::register_all(&mut registry).expect("register_all");
    let registry = registry.finish().expect("finish");

    let views: Vec<&str> = registry.views().keys().map(String::as_str).collect();
    assert_eq!(
        views,
        vec!["plane.app.views.timezone.base.TimezoneEndpoint"],
        "only the worked example is ported; update this list, ROUTES.md and the \
         group's scenario together"
    );

    let view = registry
        .view("plane.app.views.timezone.base.TimezoneEndpoint")
        .expect("timezones");
    assert!(view.has_method(&http::Method::GET));
    // `POST` is a 405 from the route table, not a handler.
    assert!(!view.has_method(&http::Method::POST));

    // `/api/unsplash/` shares the `meta` group and is not ported: it needs the
    // outbound HTTP client (DECISIONS.md D9) and must stay on Django, not answer
    // an empty list.
    assert!(
        registry
            .view("plane.app.views.external.base.UnsplashEndpoint")
            .is_none()
    );
}

#[test]
fn every_registered_view_is_on_the_app_surface() {
    // `routes.json`'s `group` is the Django URL surface (`app`, `v1`, `public`,
    // `auth`, `instances`, `web`), not the proxy route group of
    // `deploy/route-groups.yaml` (which is what this crate's directories are
    // named after). This crate serves `/api/` only: `/api/v1/`,
    // `/api/public/`, `/api/instances/` and `/auth/` have their own crates.
    let policy = PolicyTable::load().expect("routes.json");
    let mut registry = Registry::new(policy).expect("registry");
    plane_http_app::register_all(&mut registry).expect("register_all");
    let registry = registry.finish().expect("finish");

    for view in registry.views().keys() {
        let routes: Vec<&str> = registry
            .policy()
            .routes()
            .iter()
            .filter(|route| &route.view == view)
            .map(|route| route.group.as_str())
            .collect();
        assert!(!routes.is_empty(), "{view} has no route in routes.json");
        for group in routes {
            assert_eq!(
                group, "app",
                "{view} is on the {group:?} surface, which is not served by plane-http-app"
            );
        }
    }
}

#[test]
fn every_domain_registers_cleanly() {
    // One registry per domain, so a `register` that fails is named rather than
    // hidden inside `register_all`'s first error.
    macro_rules! check {
        ($($domain:ident),* $(,)?) => {
            $(
                {
                    let policy = PolicyTable::load().expect("routes.json");
                    let mut registry = Registry::new(policy).expect("registry");
                    plane_http_app::$domain::register(&mut registry)
                        .expect(concat!(stringify!($domain), "::register"));
                }
            )*
        };
    }
    check!(
        meta,
        users_me,
        assets,
        workspaces,
        analytics,
        projects,
        states,
        labels,
        estimates,
        issues,
        cycles,
        modules,
        views,
        pages,
        intake,
        notifications,
        search,
        stickies,
        webhooks,
        ai,
    );
}
