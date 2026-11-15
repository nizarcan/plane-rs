// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The per-request database connection.
//!
//! Django opens a connection on the first query and keeps it for the request.
//! The pool is chosen by `ReadReplicaRoutingMiddleware` +
//! `plane/utils/core/dbrouters.py`: reads on a view that sets
//! `use_read_replica=True` go to the replica, everything else to the primary.
//! [`DbPools::read_pool`] implements that rule, so a single connection is
//! enough for the requests this runtime serves (Phase 2 performs reads only:
//! session, user, permissions, and the space deploy-board anchor check).
//!
//! A later write inside a handler must check out a primary connection of its
//! own through `plane-db`; the handle here is deliberately read-only in shape.

use deadpool_postgres::Client;
use http::Method;
use plane_db::DbPools;

use crate::error::ViewError;

/// The lazily checked-out connection for one request.
pub struct Conn<'a> {
    pools: &'a DbPools,
    method: Method,
    use_read_replica: bool,
    client: Option<Client>,
}

impl<'a> Conn<'a> {
    pub fn new(pools: &'a DbPools, method: Method, use_read_replica: bool) -> Self {
        Self {
            pools,
            method,
            use_read_replica,
            client: None,
        }
    }

    /// The connection, opened on first use with the read/write routing rule.
    pub async fn client(&mut self) -> Result<&Client, ViewError> {
        if self.client.is_none() {
            let client = self
                .pools
                .read_pool(&self.method, self.use_read_replica)
                .get()
                .await
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
            self.client = Some(client);
        }
        match &self.client {
            Some(client) => Ok(client),
            // Unreachable: the branch above either set it or returned.
            None => Err(ViewError::Unknown("no database connection".to_string())),
        }
    }

    /// Whether a connection was opened (tests prove lazy behaviour).
    pub fn is_open(&self) -> bool {
        self.client.is_some()
    }
}
