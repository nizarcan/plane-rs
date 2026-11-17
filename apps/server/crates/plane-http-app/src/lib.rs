// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-http-app`: every handler for the `/api/` surface.
//!
//! One module per route group of `apps/server/deploy/route-groups.yaml`. The
//! group boundaries are the proxy's, so a group can be moved to Rust (or sent
//! back to Django) as a unit, and they are also the unit of porting: one agent,
//! one directory, one difftest scenario.
//!
//! ## Layout
//!
//! ```text
//! src/lib.rs                 `register_all`, `AppState` (shared, orchestrator-owned)
//! src/common/                shared helpers (shared, orchestrator-owned)
//! src/<group>/mod.rs         `register`: the group's `Registry` registrations
//! src/<group>/queries.rs     the group's static SQL catalog (`ALL_QUERIES`)
//! src/<group>/*.rs           one file per view or small family of views
//! tests/schema_check.rs      prepares every catalog above against Postgres
//! tests/registry.rs          proves `register_all` registers only what it says
//! ```
//!
//! A domain agent edits only its own `src/<group>/` directory: it adds handlers
//! and `plane_query!` statements inside that directory, lists the statements in
//! its own `queries::ALL_QUERIES`, and calls its handlers from its own
//! `register`. `lib.rs`, `src/common/`, the two tests and `Cargo.toml` are
//! shared and change by review, never to get one route in.
//!
//! ## What is registered today
//!
//! One route: `GET /api/timezones/` (`src/meta/timezones.rs`), the worked
//! example `apps/server/HANDLERS.md` walks through. Everything else in
//! `/api/` still answers the pipeline's 501, which is what the coexistence
//! proxy expects while Django serves it. A group's routes are registered as
//! they are ported; "registered" and "verified" are separate, and only a clean
//! scenario makes a route verified (`HANDLERS.md` §8).
//!
//! `register_all` is called by `bins/plane-api/src/startup.rs` after the
//! `plane.web` views and before `Registry::finish`.

pub mod ai;
pub mod analytics;
pub mod assets;
pub mod auth;
pub mod common;
pub mod cycles;
pub mod estimates;
pub mod intake;
pub mod issues;
pub mod labels;
pub mod meta;
pub mod modules;
pub mod notifications;
pub mod pages;
pub mod projects;
pub mod search;
pub mod states;
pub mod stickies;
pub mod users_me;
pub mod views;
pub mod webhooks;
pub mod workspaces;
pub mod write;

use plane_core::config::Settings;
use plane_db::DbPools;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::ViewCtx;

/// Register every ported `/api/` handler.
///
/// The order is the porting order in `deploy/route-groups.yaml` and is fixed
/// here rather than derived, so a second call, a missing call or a reordering
/// shows up as a diff in this file. Dispatch does not depend on it (the route
/// table decides), but registration is stateful — a handler registered twice
/// is a `RegistryError` — so one place owns the sequence.
///
/// A group with nothing ported yet returns `Ok(())` and registers nothing;
/// that is deliberate and is what keeps its routes on Django.
pub fn register_all(registry: &mut Registry) -> Result<(), RegistryError> {
    meta::register(registry)?;
    users_me::register(registry)?;
    assets::register(registry)?;
    workspaces::register(registry)?;
    analytics::register(registry)?;
    projects::register(registry)?;
    states::register(registry)?;
    labels::register(registry)?;
    estimates::register(registry)?;
    issues::register(registry)?;
    cycles::register(registry)?;
    modules::register(registry)?;
    views::register(registry)?;
    pages::register(registry)?;
    intake::register(registry)?;
    notifications::register(registry)?;
    search::register(registry)?;
    stickies::register(registry)?;
    webhooks::register(registry)?;
    ai::register(registry)?;
    auth::register(registry)?;
    Ok(())
}

/// The shared, non-request state behind one `/api/` request.
///
/// `plane-drf`'s [`ViewCtx`] already carries every per-request resource — the
/// user, the activated timezone, the routed connection, the pools, the settings
/// and the process's cache and Celery handles — so this type repeats none of
/// them. It exists so a helper in [`common`] can take the resources it needs
/// without taking the whole request. `plane-http-app` holds no process state of
/// its own: the handles live in the runtime (`bins/plane-api`).
#[derive(Debug, Clone, Copy)]
pub struct AppState<'a> {
    /// The primary and optional replica pools (`ViewCtx::pools`).
    pub pools: &'a DbPools,
    /// The parsed environment (`ViewCtx::settings`), for the few handlers that
    /// read configuration instead of a model column.
    pub settings: &'a Settings,
}

impl<'a> AppState<'a> {
    /// The resources behind one request.
    pub fn from_ctx(ctx: &'a ViewCtx<'a>) -> Self {
        Self {
            pools: ctx.pools,
            settings: ctx.settings,
        }
    }
}
