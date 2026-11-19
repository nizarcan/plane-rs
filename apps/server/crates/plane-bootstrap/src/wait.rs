// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two readiness gates every binary runs before touching the database.
//!
//! `wait_for_db` is `plane/db/management/commands/wait_for_db.py:14-24`. Django's
//! command has a bug: `connections["default"]` returns a lazy wrapper and never
//! opens a connection, so the loop never retries and a database that is down is
//! reported available (`wait_for_db.py:17-22`). The plan asks for a real
//! connect-retry helper here (`plans/CUTOVER.md` §3), so this actually connects;
//! the messages, including the `waititng` typo, are Django's.
//!
//! `wait_for_migrations` replaces
//! `plane/db/management/commands/wait_for_migrations.py:14-26`: Django asks its
//! migration graph whether anything is pending; after cutover the graph lives in
//! `plane_jobs.meta`, and the marker the migrator writes
//! ([`crate::instance`]'s baseline id and `schema_version`) is the readiness
//! signal. It also does not migrate: only `plane-migrator` does
//! (`plans/CUTOVER.md` §3).

use std::time::Duration;

use tokio_postgres::Client;

use plane_db::DbPools;

use crate::Result;

/// Seconds between failed connection attempts (`time.sleep(1)`).
const DB_RETRY: Duration = Duration::from_secs(1);
/// Seconds between migration checks (`time.sleep(10)`).
const MIGRATION_RETRY: Duration = Duration::from_secs(10);

/// `wait_for_db`: print Django's two lines, retrying until the pool hands out a
/// connection.
pub async fn wait_for_db(pools: &DbPools) -> Result<()> {
    println!("Waiting for database...");
    loop {
        match pools.primary().get().await {
            Ok(connection) => {
                drop(connection);
                println!("Database available!");
                return Ok(());
            }
            Err(error) => {
                println!("Database unavailable, waititng 1 second...");
                tracing::warn!(target: "plane.boot", %error, "database is not accepting connections yet");
                tokio::time::sleep(DB_RETRY).await;
            }
        }
    }
}

/// `wait_for_migrations`: wait until `plane-migrator` has created the
/// `plane_jobs` schema and its `django_baseline` marker.
///
/// A missing schema, a missing marker and a connection error are all "not
/// ready": the migrator may still be running. The process prints Django's
/// waiting line every ten seconds and the success line once.
pub async fn wait_for_migrations(pools: &DbPools) -> Result<()> {
    loop {
        let ready = match pools.primary().get().await {
            Ok(client) => migrations_ready(&client).await,
            Err(error) => {
                tracing::warn!(target: "plane.boot", %error, "database is not accepting connections yet");
                Ok(false)
            }
        };
        match ready {
            Ok(true) => {
                println!("No migrations Pending. Starting processes ...");
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(target: "plane.boot", %error, "reading plane_jobs.meta failed; still waiting");
            }
        }
        println!("Waiting for database migrations to complete...");
        tokio::time::sleep(MIGRATION_RETRY).await;
    }
}

/// The marker `plane-migrator` writes after the baseline and the `plane_jobs`
/// DDL. Both keys must exist: `django_baseline` proves the Django schema was
/// provisioned, `schema_version` proves the queue DDL ran.
async fn migrations_ready(client: &Client) -> Result<bool> {
    let exists = client
        .query_one("SELECT to_regclass('plane_jobs.meta') IS NOT NULL", &[])
        .await?
        .get::<_, bool>(0);
    if !exists {
        return Ok(false);
    }
    let baseline = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM plane_jobs.meta WHERE key = 'django_baseline')",
            &[],
        )
        .await?
        .get::<_, bool>(0);
    if !baseline {
        return Ok(false);
    }
    let row = client
        .query_opt(
            "SELECT value FROM plane_jobs.meta WHERE key = 'schema_version'",
            &[],
        )
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let found: String = row.get(0);
    // The expected value comes from the `plane_jobs` crate that also writes it
    // (`queue::SCHEMA_VERSION`); a consumer never hard-codes a version. A
    // different one means the migrator this process depends on is another
    // build: keep waiting rather than run against a queue it does not know.
    if found != plane_jobs::queue::SCHEMA_VERSION {
        tracing::warn!(
            target: "plane.boot",
            found = %found,
            expected = plane_jobs::queue::SCHEMA_VERSION,
            "plane_jobs schema_version does not match this binary; waiting"
        );
        return Ok(false);
    }
    Ok(true)
}
