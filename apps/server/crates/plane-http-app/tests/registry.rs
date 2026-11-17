// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Invariants of `register_all` that survive a domain being merged.
//!
//! The former assertion was an exact list of registered views, which had to be
//! edited in the same commit as `ROUTES.md` and the group's scenario. That is
//! correct but it stops being writable as soon as several domains land at once,
//! and it never expressed *why* the list mattered. These four invariants do,
//! and none of them mentions a specific route:
//!
//! 1. `register_all` succeeds on a fresh [`Registry`] built from the embedded
//!    policy. Registration is stateful and one place owns the sequence, so a
//!    duplicated or stale registration is a startup failure and must stay one.
//! 2. Every registered `(view, action or method)` resolves to a route whose
//!    `routes.json` `group` is `app`. This crate serves `/api/` only; a handler
//!    for `/api/v1/`, `/api/public/`, `/api/instances/` or `/auth/` would be
//!    dead code in the wrong binary.
//! 3. Each domain module registers an action or method only if every route that
//!    dispatches to it belongs to that module's group in
//!    `deploy/route-groups.yaml` (`src/lib.rs` maps module to group: `users_me`
//!    → `users-me`). The check is per registered `(route, action or method)`
//!    pair, not per view: one Django view can serve routes in several groups
//!    (`ModuleIssueViewSet` serves `modules/{module_id}/issues/` with
//!    `list`/`retrieve` and `issues/{issue_id}/modules/` with
//!    `create_issue_modules`). The group is the proxy's unit of rollback, so a
//!    handler in the wrong directory moves a route with the wrong flag.
//! 4. Proxy safety: every `(route, method)` the Owner column of `ROUTES.md`
//!    marks `rust` or `rust(GET,…)` is registered. The coexistence proxy emits
//!    a matcher for exactly those pairs, so an unregistered one is a request
//!    the proxy sends to Rust that answers the pipeline's 501. A registered
//!    route still owned by `django` is fine: ported but not yet flipped.
//!
//! `ROUTES.md` is parsed with the same grammar `tools/inventory/routes.py`
//! (`parse_ownership`, `parse_owner_cell`) defines and `tools/proxy/gen_caddy.py`
//! consumes: `## app (`…`)`-style group headings, `|`-separated rows with
//! `\|` escapes, and an Owner cell that is `django`, `rust` or `rust(GET,HEAD)`.
//! The group assignment is a port of `assign_groups` from `gen_caddy.py`
//! (exact selectors first, then the most specific glob; a tie or a miss is an
//! error), so the test and the generator cannot disagree.
//!
//! Set `PLANE_RS_ROUTES_MD` to parse a copy of `ROUTES.md` instead of the real
//! one (default: `<repo>/apps/server/ROUTES.md`). That is how invariant 4 is
//! shown to fail: mark an unregistered `/api/` route `rust(GET)` in the copy and
//! the test names the route, the method and the module.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use http::Method;
use plane_drf::policy::{MethodPolicy, PolicyTable, RoutePolicy, method_from_name};
use plane_drf::registry::Registry;

/// Every module `register_all` calls, in `src/lib.rs`'s order, with the
/// `deploy/route-groups.yaml` group whose routes that module must serve.
const DOMAINS: &[(&str, &str)] = &[
    // The `/auth/` session group lives in this crate by the orchestrator's
    // instruction (`AUTH_FLOWS.md` set A registers it here, not in a separate
    // `plane-http-auth` crate), so it is a domain like any other.
    ("auth", "auth-session"),
    ("meta", "meta"),
    ("users_me", "users-me"),
    ("assets", "assets"),
    ("workspaces", "workspaces"),
    ("analytics", "analytics"),
    ("projects", "projects"),
    ("states", "states"),
    ("labels", "labels"),
    ("estimates", "estimates"),
    ("issues", "issues"),
    ("cycles", "cycles"),
    ("modules", "modules"),
    ("views", "views"),
    ("pages", "pages"),
    ("intake", "intake"),
    ("notifications", "notifications"),
    ("search", "search"),
    ("stickies", "stickies"),
    ("webhooks", "webhooks"),
    ("ai", "ai"),
];

/// `apps/server/`, from `CARGO_MANIFEST_DIR` (`apps/server/crates/plane-http-app`).
fn server_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn policy() -> PolicyTable {
    PolicyTable::load().expect("apps/server/routes.json must load")
}

/// A fresh registry with `register_all` applied, or a panic naming the step.
fn register_all_registry() -> Registry {
    let mut registry = Registry::new(policy()).expect("Registry::new(routes.json)");
    plane_http_app::register_all(&mut registry).expect("plane_http_app::register_all");
    registry
        .finish()
        .expect("Registry::finish after register_all")
}

/// One fresh registry per domain, so a view can be attributed to a module.
///
/// The registry is a `BTreeMap` of views and has no `Clone`, so this builds the
/// policy again per module: `routes.json` is embedded, and the cost is a few
/// milliseconds in a test, against the alternative of a shared registry where a
/// duplicate registration would hide which module failed.
fn domain_registries() -> Vec<(&'static str, &'static str, Registry)> {
    let mut registries = Vec::with_capacity(DOMAINS.len());
    for (module, group) in DOMAINS {
        let mut registry = Registry::new(policy()).expect("Registry::new(routes.json)");
        register_module(module)(&mut registry).unwrap_or_else(|error| {
            panic!("{module}::register on a fresh registry failed: {error}")
        });
        let registry = registry.finish().unwrap_or_else(|error| {
            panic!("{module}::register left an unusable registry: {error}")
        });
        registries.push((*module, *group, registry));
    }
    registries
}

/// The module's `register`, selected by name because `DOMAINS` is a table.
/// There is no reflection: `DOMAINS` and this `match` must be extended together
/// with `src/lib.rs`, and `register_all` is checked by invariant 1 regardless.
fn register_module(module: &str) -> RegisterFn {
    match module {
        "auth" => plane_http_app::auth::register,
        "meta" => plane_http_app::meta::register,
        "users_me" => plane_http_app::users_me::register,
        "assets" => plane_http_app::assets::register,
        "workspaces" => plane_http_app::workspaces::register,
        "analytics" => plane_http_app::analytics::register,
        "projects" => plane_http_app::projects::register,
        "states" => plane_http_app::states::register,
        "labels" => plane_http_app::labels::register,
        "estimates" => plane_http_app::estimates::register,
        "issues" => plane_http_app::issues::register,
        "cycles" => plane_http_app::cycles::register,
        "modules" => plane_http_app::modules::register,
        "views" => plane_http_app::views::register,
        "pages" => plane_http_app::pages::register,
        "intake" => plane_http_app::intake::register,
        "notifications" => plane_http_app::notifications::register,
        "search" => plane_http_app::search::register,
        "stickies" => plane_http_app::stickies::register,
        "webhooks" => plane_http_app::webhooks::register,
        "ai" => plane_http_app::ai::register,
        other => panic!("DOMAINS lists {other:?}, which register_all does not call"),
    }
}

type RegisterFn = fn(&mut Registry) -> Result<(), plane_drf::registry::RegistryError>;

/// Whether a route answers `entry` (an action for a ViewSet, a method name
/// otherwise) per its policy.
fn route_answers(route: &RoutePolicy, entry: &str, is_viewset: bool) -> bool {
    route.methods.iter().any(|policy| {
        if is_viewset {
            policy.action.as_deref() == Some(entry)
        } else {
            policy.method.as_str().eq_ignore_ascii_case(entry)
        }
    })
}

// ---------------------------------------------------------------------------
// 1. register_all succeeds
// ---------------------------------------------------------------------------

#[test]
fn register_all_succeeds_on_a_fresh_registry() {
    let registry = register_all_registry();
    assert!(
        !registry.views().is_empty(),
        "register_all registered no view: every /api/ route would answer 501, including the \
         ones ROUTES.md already marks rust"
    );
}

// ---------------------------------------------------------------------------
// 2. only the /api/ (app) surface
// ---------------------------------------------------------------------------

#[test]
fn only_app_routes_are_registered() {
    let registry = register_all_registry();
    for (view_name, view) in registry.views() {
        let is_viewset = view.family.is_viewset();
        let entries = view.registered();
        assert!(
            !entries.is_empty(),
            "{view_name} is registered with no handler; remove the empty registration"
        );
        for entry in entries {
            let matching: Vec<&RoutePolicy> = registry
                .policy()
                .routes()
                .iter()
                .filter(|route| route.view == *view_name)
                .filter(|route| route_answers(route, &entry, is_viewset))
                .collect();
            assert!(
                !matching.is_empty(),
                "{view_name} registers {entry}, but no routes.json row for that view maps it"
            );
            for route in matching {
                // `/api/` is the `app` group; the session handlers (`set A`)
                // are the `auth` group and are registered here on the
                // orchestrator's instruction. Every other surface has its own
                // crate.
                assert!(
                    route.group == "app" || route.group == "auth",
                    "{view_name} registers {entry} for {}, but routes.json puts that route on \
                     the {:?} surface; plane-http-app serves /api/ (and the /auth/ session \
                     group), and the other surfaces have their own crates",
                    route.path_template,
                    route.group
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 3. each module serves its own route group
// ---------------------------------------------------------------------------

#[test]
fn each_domain_registers_only_its_route_group() {
    let groups = load_route_groups();

    // The parser's own check, which `gen_caddy.py` also enforces: every route
    // in routes.json belongs to exactly one group. A selector this test fails
    // to parse therefore shows up here instead of as a silently weaker
    // per-domain check.
    let policy = policy();
    for route in policy.routes() {
        assign_group(&groups, &route.group, &route.path_template).unwrap_or_else(|error| {
            panic!("deploy/route-groups.yaml does not assign every routes.json route: {error}")
        });
    }

    for (module, group_name, registry) in domain_registries() {
        check_domain_groups(&groups, module, group_name, &registry);
    }
}

/// Invariant 3 for one domain: every `(view, action or method)` it registered
/// must be dispatched to only by routes in the module's group.
///
/// The unit is the registered entry, not the view. `ModuleIssueViewSet` is
/// served from two groups with disjoint actions, so a per-view check would
/// refuse the `modules` module for a view that also owns an `issues` route,
/// even though the actions it registers never reach that route.
fn check_domain_groups(groups: &[RouteGroup], module: &str, group_name: &str, registry: &Registry) {
    for (view_name, view) in registry.views() {
        let is_viewset = view.family.is_viewset();
        let kind = if is_viewset { "action" } else { "method" };
        for entry in view.registered() {
            // The routes whose own policy dispatches to this action/method;
            // that is exactly how `RegisteredView::handler` resolves a request.
            let routes: Vec<&RoutePolicy> = registry
                .policy()
                .routes()
                .iter()
                .filter(|route| route.view == *view_name)
                .filter(|route| route_answers(route, &entry, is_viewset))
                .collect();
            assert!(
                !routes.is_empty(),
                "{module}::register registers {kind} {entry:?} on {view_name}, but no routes.json \
                 row for that view dispatches to it"
            );
            for route in routes {
                let assigned = assign_group(groups, &route.group, &route.path_template)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{module}::register registers {kind} {entry:?} on {view_name}: {error}"
                        )
                    });
                assert_eq!(
                    assigned, group_name,
                    "{module}::register registers {kind} {entry:?} on {view_name}, but the route \
                     that dispatches it, {} ({}) is the {assigned:?} group in \
                     deploy/route-groups.yaml, not {group_name:?}; move the handler to the \
                     {assigned} module or fix the group's selectors",
                    route.path_template, route.group
                );
            }
        }
    }
}

/// Shown to fail: the `modules` module registering `create_issue_modules`,
/// whose only route lives in the `issues` group. A test-only fixture, so no
/// domain module is edited to demonstrate the check.
#[test]
fn invariant_3_rejects_an_action_whose_route_is_in_another_group() {
    let groups = load_route_groups();
    let mut registry = Registry::new(policy()).expect("Registry::new(routes.json)");
    registry
        .viewset("plane.app.views.module.issue.ModuleIssueViewSet")
        .expect("ModuleIssueViewSet is a registered ViewSet")
        .action("create_issue_modules", plane_http_app::modules::issue::list)
        .expect("create_issue_modules is a policy action");
    let registry = registry.finish().expect("finish");

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check_domain_groups(&groups, "modules", "modules", &registry);
    }))
    .expect_err("an action whose route is in another group must fail invariant 3");
    let message = failure
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            failure
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
        })
        .expect("invariant 3 panics with a formatted message");

    for expected in [
        "modules::register registers action \"create_issue_modules\" on \
         plane.app.views.module.issue.ModuleIssueViewSet",
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/ (app)",
        "is the \"issues\" group in deploy/route-groups.yaml, not \"modules\"",
    ] {
        assert!(
            message.contains(expected),
            "invariant 3's message does not name {expected:?}: {message}"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. proxy safety: everything marked rust is registered
// ---------------------------------------------------------------------------

#[test]
fn every_rust_owned_app_route_method_is_registered() {
    let registry = register_all_registry();
    let groups = load_route_groups();

    // view -> module, and group -> module, for the failure messages.
    let domains = domain_registries();
    let mut view_module: BTreeMap<&str, &str> = BTreeMap::new();
    for (module, _group, domain_registry) in &domains {
        for view_name in domain_registry.views().keys() {
            view_module.insert(view_name.as_str(), module);
        }
    }
    let group_module: BTreeMap<&str, &str> = DOMAINS.iter().copied().collect();

    let rows = load_routes_md();
    let mut checked = 0usize;
    for row in rows.iter().filter(|row| row.group == "app") {
        let (engine, owner_methods) = parse_owner_cell(&row.owner).unwrap_or_else(|error| {
            panic!(
                "{}: invalid Owner cell {:?}: {error}",
                row.location, row.owner
            )
        });
        if engine != Engine::Rust {
            // Ported but not flipped: the proxy still sends it to Django.
            continue;
        }

        let routes: Vec<&RoutePolicy> = registry
            .policy()
            .routes()
            .iter()
            .filter(|route| route.group == "app")
            .filter(|route| route.view == row.view && route.path_template == row.path)
            .collect();
        assert!(
            !routes.is_empty(),
            "{}: ROUTES.md marks {} as rust, but no app route in routes.json has that path and \
             view; regenerate ROUTES.md (`tools/inventory/routes.py`)",
            row.location,
            row.path
        );

        for route in routes {
            let expected: Vec<Method> = match &owner_methods {
                Some(methods) => methods.clone(),
                // A whole-row `rust` covers every registered handler; `HEAD`
                // (from `GET`) and `OPTIONS` (from the policy) are implicit.
                None => route
                    .methods
                    .iter()
                    .map(|policy| policy.method.clone())
                    .collect(),
            };
            for method in expected {
                let entry: Option<&MethodPolicy> =
                    route.methods.iter().find(|policy| policy.method == method);
                let registered_view = registry.view(&row.view);
                let covered = match entry {
                    Some(entry) => match registered_view {
                        Some(view) if view.family.is_viewset() => entry
                            .action
                            .as_deref()
                            .is_some_and(|action| view.registered().iter().any(|it| it == action)),
                        Some(view) => view.has_method(&method),
                        None => false,
                    },
                    // `HEAD` falls back to `GET` (`View.setup`); DRF answers
                    // `OPTIONS` from the policy, so neither needs a handler.
                    None if method == Method::HEAD => {
                        registered_view.is_some_and(|view| view.has_method(&Method::GET))
                    }
                    None if method == Method::OPTIONS => {
                        registered_view.is_some_and(|view| view.family.is_drf())
                    }
                    None => false,
                };
                if covered {
                    checked += 1;
                    continue;
                }

                let method_name = method.as_str().to_uppercase();
                let group = assign_group(&groups, &route.group, &route.path_template).ok();
                let module = view_module.get(row.view.as_str()).copied();
                let group_owner = group.and_then(|group| group_module.get(group).copied());
                let source = match (module, group_owner) {
                    (Some(module), _) => format!("the view is registered by the {module} module"),
                    (None, Some(module)) => format!(
                        "no module registers the view; route group {:?} belongs to the {module} \
                         module",
                        group.unwrap_or("?")
                    ),
                    (None, None) => "no module registers the view".to_string(),
                };
                let detail = match (entry, registered_view) {
                    (Some(entry), Some(view)) if view.family.is_viewset() => format!(
                        "the policy maps {method_name} to action {:?} on a registered view, but \
                         that action is not registered",
                        entry.action.as_deref().unwrap_or("?")
                    ),
                    (Some(_), Some(_)) => {
                        format!("the view is registered, but not for {method_name}")
                    }
                    (Some(_), None) => "the view is not registered at all".to_string(),
                    (None, _) => format!("routes.json has no {method_name} handler for this route"),
                };

                panic!(
                    "{}: ROUTES.md marks {method_name} on {} ({}) as rust, but no handler is \
                     registered: {detail}; {source}. The proxy would send {method_name} {} to \
                     Rust, which answers 501. Register the handler in the module that owns the \
                     route group, or put the Owner cell back to django.",
                    row.location, row.path, row.view, row.path
                );
            }
        }
    }

    assert!(
        checked > 0,
        "no app route in ROUTES.md is marked rust, so this test proved nothing; if the port has \
         really been rolled back to Django, delete this test with that commit"
    );
}

// ---------------------------------------------------------------------------
// `deploy/route-groups.yaml` (the subset gen_caddy.py uses)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RouteGroup {
    name: String,
    /// Exact `(routes.json group, path)` selectors.
    exact: Vec<(String, String)>,
    /// Path globs: `*` is one segment, a trailing `**` is zero or more.
    globs: Vec<String>,
}

/// Parse the group file strictly: an unrecognised line is a failure, never a
/// silent skip, so a change to the YAML shape cannot quietly weaken
/// invariant 3.
fn load_route_groups() -> Vec<RouteGroup> {
    let path = server_dir().join("deploy/route-groups.yaml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

    let mut groups: Vec<RouteGroup> = Vec::new();
    #[derive(Clone, Copy, PartialEq)]
    enum Section {
        None,
        Exact,
        Glob,
    }
    let mut section = Section::None;

    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let where_ = format!("{}:{line_no}", path.display());
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if indent == 0 {
            assert!(
                trimmed == "version: 1" || trimmed == "groups:",
                "{where_}: unrecognised top-level line {trimmed:?}; teach tests/registry.rs the \
                 new deploy/route-groups.yaml shape"
            );
            continue;
        }
        if indent == 2 {
            let entry = trimmed
                .strip_prefix("- ")
                .unwrap_or_else(|| panic!("{where_}: expected `- name: <group>`, got {trimmed:?}"));
            let name = entry
                .strip_prefix("name: ")
                .unwrap_or_else(|| panic!("{where_}: expected `- name: <group>`, got {trimmed:?}"));
            let name = name.trim().to_string();
            assert!(!name.is_empty(), "{where_}: empty group name");
            section = Section::None;
            groups.push(RouteGroup {
                name,
                exact: Vec::new(),
                globs: Vec::new(),
            });
            continue;
        }
        let group = groups
            .last_mut()
            .unwrap_or_else(|| panic!("{where_}: group property before any `- name:`"));
        if indent == 4 {
            if trimmed == "routes:" {
                section = Section::Exact;
            } else if trimmed == "match:" {
                section = Section::Glob;
            } else if let Some(description) = trimmed.strip_prefix("description: ") {
                assert!(
                    !description.trim().is_empty(),
                    "{where_}: group {:?} has an empty description",
                    group.name
                );
            } else {
                panic!(
                    "{where_}: unrecognised key in group {:?}: {trimmed:?}",
                    group.name
                );
            }
            continue;
        }
        assert_eq!(indent, 6, "{where_}: unindented selector {trimmed:?}");
        match section {
            Section::Exact => {
                let body = trimmed
                    .strip_prefix("- {")
                    .and_then(|rest| rest.strip_suffix('}'))
                    .unwrap_or_else(|| {
                        panic!(
                            "{where_}: expected `- {{ group: <surface>, path: <path> }}`, got \
                             {trimmed:?}"
                        )
                    });
                let mut surface = None;
                let mut path_value = None;
                for part in body.split(',').map(str::trim) {
                    if let Some(value) = part.strip_prefix("group: ") {
                        surface = Some(unquote(value));
                    } else if let Some(value) = part.strip_prefix("path: ") {
                        path_value = Some(unquote(value));
                    } else {
                        panic!("{where_}: unrecognised exact selector field {part:?}");
                    }
                }
                let surface = surface.unwrap_or_else(|| panic!("{where_}: selector without group"));
                let path_value =
                    path_value.unwrap_or_else(|| panic!("{where_}: selector without path"));
                group.exact.push((surface, path_value));
            }
            Section::Glob => {
                let pattern = trimmed.strip_prefix("- ").unwrap_or_else(|| {
                    panic!("{where_}: expected `- \"<glob>\"`, got {trimmed:?}")
                });
                let pattern = unquote(pattern);
                assert!(
                    pattern.starts_with('/'),
                    "{where_}: glob {pattern:?} must start with /"
                );
                group.globs.push(pattern);
            }
            Section::None => panic!(
                "{where_}: selector before `routes:`/`match:` in group {:?}: {trimmed:?}",
                group.name
            ),
        }
    }

    assert!(!groups.is_empty(), "{}: no groups parsed", path.display());
    let mut names = BTreeSet::new();
    for group in &groups {
        assert!(
            names.insert(group.name.clone()),
            "{}: duplicate group {:?}",
            path.display(),
            group.name
        );
        assert!(
            !group.exact.is_empty() || !group.globs.is_empty(),
            "{}: group {:?} selects no routes",
            path.display(),
            group.name
        );
    }
    for (module, group_name) in DOMAINS {
        assert!(
            names.contains(*group_name),
            "{}: module {module} maps to group {group_name:?}, which the file does not define",
            path.display()
        );
    }
    groups
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

// Port of `path_segments` / `glob_matches` / `selector_score` / `assign_groups`
// from `apps/server/tools/proxy/gen_caddy.py`. The rules are documented at the
// top of `deploy/route-groups.yaml`.

fn path_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        return Vec::new();
    }
    path.trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .collect()
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern_segments = path_segments(pattern);
    let path_segments = path_segments(path);
    for (index, segment) in pattern_segments.iter().enumerate() {
        if *segment == "**" {
            return index == pattern_segments.len() - 1 && path_segments.len() >= index;
        }
        let matched = path_segments
            .get(index)
            .is_some_and(|path_segment| *segment == "*" || segment == path_segment);
        if !matched {
            return false;
        }
    }
    path_segments.len() == pattern_segments.len()
}

/// More literal segments, then fewer `**`, then fewer `*`, then longer.
/// A port of `selector_score` in `tools/proxy/gen_caddy.py`.
type SelectorScore = (u32, i32, i32, u32);

/// `(score, group name, pattern)` for one matching glob.
type Candidate<'a> = (SelectorScore, &'a str, &'a str);

fn selector_score(pattern: &str) -> SelectorScore {
    let segments = path_segments(pattern);
    let literals = segments
        .iter()
        .filter(|segment| **segment != "*" && **segment != "**")
        .count() as u32;
    let deep = -(segments.iter().filter(|segment| **segment == "**").count() as i32);
    let wild = -(segments.iter().filter(|segment| **segment == "*").count() as i32);
    (literals, deep, wild, pattern.len() as u32)
}

/// The one group a route belongs to, or the reason the generator would refuse.
fn assign_group<'a>(
    groups: &'a [RouteGroup],
    surface_group: &str,
    path: &str,
) -> Result<&'a str, String> {
    let exact: Vec<&RouteGroup> = groups
        .iter()
        .filter(|group| {
            group
                .exact
                .iter()
                .any(|(surface, exact_path)| surface == surface_group && exact_path == path)
        })
        .collect();
    match exact.as_slice() {
        [] => {}
        [only] => return Ok(&only.name),
        many => {
            let names: Vec<&str> = many.iter().map(|group| group.name.as_str()).collect();
            return Err(format!(
                "route {path} ({surface_group}) is listed exactly by {}",
                names.join(", ")
            ));
        }
    }

    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    for group in groups {
        for pattern in &group.globs {
            if glob_matches(pattern, path) {
                candidates.push((selector_score(pattern), &group.name, pattern));
            }
        }
    }
    let Some(best) = candidates.iter().map(|candidate| candidate.0).max() else {
        return Err(format!(
            "route {path} ({surface_group}) belongs to no group in deploy/route-groups.yaml"
        ));
    };
    let winners: Vec<&Candidate<'_>> = candidates
        .iter()
        .filter(|candidate| candidate.0 == best)
        .collect();
    let names: BTreeSet<&str> = winners.iter().map(|candidate| candidate.1).collect();
    if names.len() > 1 {
        let detail: Vec<String> = winners
            .iter()
            .map(|candidate| format!("{} via {:?}", candidate.1, candidate.2))
            .collect();
        return Err(format!(
            "route {path} ({surface_group}) is matched equally well by {}; make one selector more \
             specific",
            detail.join(", ")
        ));
    }
    Ok(winners[0].1)
}

// ---------------------------------------------------------------------------
// `ROUTES.md` (the Owner column)
// ---------------------------------------------------------------------------

struct RoutesMdRow {
    group: String,
    path: String,
    view: String,
    owner: String,
    /// `ROUTES.md:<line>`, for messages.
    location: String,
}

fn routes_md_path() -> PathBuf {
    match std::env::var_os("PLANE_RS_ROUTES_MD") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => server_dir().join("ROUTES.md"),
    }
}

/// `parse_ownership` in `tools/inventory/routes.py`: group headings, table
/// rows, cells split on unescaped `|`, backticked values unquoted.
fn load_routes_md() -> Vec<RoutesMdRow> {
    let path = routes_md_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));

    let mut rows = Vec::new();
    let mut group: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        if let Some(heading) = line.strip_prefix("## ") {
            // `## app (`/api/`)`, `## Phase 2 policy fields …`, `## Summary`:
            // the group name is the leading lowercase word, as
            // `OWNERSHIP_GROUP_RE` (`^(app|v1|…)`) requires.
            let name: String = heading
                .chars()
                .take_while(char::is_ascii_lowercase)
                .collect();
            group = Some(name);
            continue;
        }
        if line.starts_with('#') {
            group = None;
            continue;
        }
        let Some(current_group) = group.as_deref() else {
            continue;
        };
        if !line.starts_with('|') {
            continue;
        }

        let cells = table_cells(line);
        if cells.len() != 7 || !cells[0].trim().chars().all(|c| c.is_ascii_digit()) {
            // Header and separator rows, and the summary table's group names.
            continue;
        }
        let path_cell = strip_cell(&cells[1]);
        let view = strip_cell(&cells[3]);
        let owner = strip_cell(&cells[5]);
        if path_cell.is_empty() || view.is_empty() || owner.is_empty() {
            continue;
        }
        rows.push(RoutesMdRow {
            group: current_group.to_string(),
            path: path_cell,
            view,
            owner,
            location: format!("{}:{}", path.display(), index + 1),
        });
    }
    rows
}

/// Split a table row on unescaped `|` and drop the empty leading/trailing cells
/// (`TABLE_SPLIT_RE` is `(?<!\\)\|`).
fn table_cells(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&'|') => {
                characters.next();
                current.push('|');
            }
            '|' => cells.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    cells.push(current);
    while cells.first().is_some_and(|cell| cell.trim().is_empty()) {
        cells.remove(0);
    }
    while cells.last().is_some_and(|cell| cell.trim().is_empty()) {
        cells.pop();
    }
    cells
}

/// `strip_cell`: trim, then drop one pair of surrounding backticks.
fn strip_cell(cell: &str) -> String {
    let value = cell.trim();
    value
        .strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or(value)
        .to_string()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Engine {
    Django,
    Rust,
}

/// `parse_owner_cell`: `django`, `rust` or `rust(GET,HEAD)`.
///
/// `Ok((Rust, None))` is a whole-row owner; `Ok((Rust, Some(methods)))` is a
/// method split. Anything else is an error, with the generator's wording.
fn parse_owner_cell(cell: &str) -> Result<(Engine, Option<Vec<Method>>), String> {
    let text = cell.trim();
    if text.is_empty() {
        return Err("owner cell is empty".to_string());
    }

    let (engine_text, method_list) = match text.split_once('(') {
        Some((engine, rest)) => {
            let list = rest.strip_suffix(')').ok_or_else(|| {
                format!("unknown owner syntax {text:?}; expected django, rust or rust(GET,HEAD)")
            })?;
            if list.contains(['(', ')']) {
                return Err(format!(
                    "unknown owner syntax {text:?}; expected django, rust or rust(GET,HEAD)"
                ));
            }
            (engine, Some(list))
        }
        None => (text, None),
    };
    if engine_text.is_empty() || !engine_text.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(format!(
            "unknown owner syntax {text:?}; expected django, rust or rust(GET,HEAD)"
        ));
    }

    match engine_text {
        "django" => {
            if method_list.is_some() {
                return Err(
                    "django cannot carry a method list; use rust(GET,HEAD) to move a subset to \
                     Rust"
                        .to_string(),
                );
            }
            Ok((Engine::Django, None))
        }
        "rust" => {
            let Some(list) = method_list else {
                return Ok((Engine::Rust, None));
            };
            let mut methods: Vec<Method> = Vec::new();
            for name in list.split(',') {
                let name = name.trim();
                let method = method_from_name(name)
                    .filter(|method| method.as_str().eq_ignore_ascii_case(name))
                    .filter(|method| method.as_str() == name)
                    .ok_or_else(|| {
                        format!(
                            "unknown method {name:?} in {text:?}; expected uppercase methods from \
                             GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, TRACE"
                        )
                    })?;
                if methods.contains(&method) {
                    return Err(format!("method {name:?} is listed twice in {text:?}"));
                }
                methods.push(method);
            }
            if methods.is_empty() {
                return Err(format!(
                    "malformed method list in {text:?}; expected rust(GET,HEAD)"
                ));
            }
            Ok((Engine::Rust, Some(methods)))
        }
        _ => Err(format!(
            "unknown owner {engine_text:?}; expected one of django, rust"
        )),
    }
}
