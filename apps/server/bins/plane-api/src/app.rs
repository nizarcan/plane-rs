// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared state for the HTTP server.

use std::sync::Arc;

use plane_core::config::Settings;
use plane_db::DbPools;

use crate::routes::RouteTable;

/// Everything a request handler can reach.
#[derive(Debug)]
pub struct AppState {
    pub settings: Settings,
    pub routes: RouteTable,
    /// Lazily connecting pools: building them opens no connection, so the
    /// process starts with Postgres down and `/` never touches it.
    ///
    /// Phase 2's handlers check connections out of this; Phase 1 constructs it
    /// so the pool configuration is exercised at startup and a bad
    /// `DATABASE_URL` fails before the listener opens.
    #[allow(dead_code)]
    pub db: DbPools,
}

pub type SharedState = Arc<AppState>;
