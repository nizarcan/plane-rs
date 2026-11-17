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
//!
//! One exception needs a *second* connection: API-key authentication. Django's
//! router sends the `api_tokens` write to `default` on every request
//! (`dbrouters.py:56-72`, `db_for_write`), so [`Conn::primary_client`] checks
//! one out from the primary pool and the token SELECT and the `last_used`
//! UPDATE both run there. Using the routed connection would write `last_used`
//! to the replica on an opted-in GET. (Django's *read* of `api_tokens` follows
//! `db_for_read` and would use the replica; the primary is the stricter
//! choice and the two agree whenever no replica is configured.)

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
    /// The primary-pool connection for `api_tokens`
    /// (`plane/api/middleware/api_authentication.py:29-43`).
    primary: Option<Client>,
}

impl<'a> Conn<'a> {
    pub fn new(pools: &'a DbPools, method: Method, use_read_replica: bool) -> Self {
        Self {
            pools,
            method,
            use_read_replica,
            client: None,
            primary: None,
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

    /// The primary-pool connection: what `api_tokens` uses.
    ///
    /// `APIKeyAuthentication` reads the token row and writes `last_used`; the
    /// write always goes to `default` (`dbrouters.py:56-72`), so both halves
    /// stay on one connection here.
    pub async fn primary_client(&mut self) -> Result<&Client, ViewError> {
        if self.primary.is_none() {
            let client = self
                .pools
                .primary()
                .get()
                .await
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
            self.primary = Some(client);
        }
        match &self.primary {
            Some(client) => Ok(client),
            // Unreachable: the branch above either set it or returned.
            None => Err(ViewError::Unknown("no database connection".to_string())),
        }
    }

    /// The pool [`Conn::primary_client`] checks out from, for tests.
    #[must_use]
    pub fn primary_pool(&self) -> &'a deadpool_postgres::Pool {
        self.pools.primary()
    }

    /// The pool [`Conn::client`] would check out from, for tests.
    #[must_use]
    pub fn routed_pool(&self) -> &'a deadpool_postgres::Pool {
        self.pools.read_pool(&self.method, self.use_read_replica)
    }

    /// Whether a connection was opened (tests prove lazy behaviour).
    pub fn is_open(&self) -> bool {
        self.client.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_core::config::{EnvMap, Role, Settings};

    fn pools(enable_replica: bool) -> DbPools {
        let env = EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set(
                "DATABASE_URL",
                "postgresql://plane:plane@127.0.0.1:55432/plane",
            );
        let env = if enable_replica {
            env.set("ENABLE_READ_REPLICA", "1").set(
                "DATABASE_READ_REPLICA_URL",
                "postgresql://plane:plane@127.0.0.1:55432/plane_replica",
            )
        } else {
            env
        };
        let settings = Settings::from_source(&env, Role::Api).expect("settings");
        DbPools::from_settings(&settings).expect("pools")
    }

    /// `APIKeyAuthentication` must not follow the read-replica routing rule:
    /// its `last_used` write always goes to `default`
    /// (`plane/utils/core/dbrouters.py:56-72`). With a replica configured, a
    /// GET on an opted-in view routes the request connection to the replica and
    /// the token connection to the primary.
    #[test]
    fn api_token_access_uses_the_primary_pool() {
        let with_replica = pools(true);
        let replica = with_replica.replica().expect("replica") as *const _ as usize;
        let primary = with_replica.primary() as *const _ as usize;

        let conn = Conn::new(&with_replica, Method::GET, true);
        assert_eq!(conn.routed_pool() as *const _ as usize, replica);
        assert_eq!(conn.primary_pool() as *const _ as usize, primary);

        // Without a replica both are the primary, so the fix cannot change a
        // single-replica deployment's behaviour.
        let single = pools(false);
        let conn = Conn::new(&single, Method::GET, true);
        assert_eq!(
            conn.routed_pool() as *const _ as usize,
            conn.primary_pool() as *const _ as usize
        );
    }
}
