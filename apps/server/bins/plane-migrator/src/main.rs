// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-migrator`: own the schema from the last Django migration on.
//!
//! Two things happen here, in this order, under one advisory lock
//! (`plans/CUTOVER.md` §4):
//!
//! 1. the Django baseline: a fresh install applies the embedded `pg_dump` and
//!    its `django_migrations` rows; an existing database must carry exactly
//!    the baseline's migration set (older and newer are both refused);
//! 2. the Rust-owned `plane_jobs` schema, which must not appear in
//!    `apps/api/migrations_lockfile.txt`.
//!
//! It is deliberately separate from Django's `manage.py migrate`.

mod schema;

use anyhow::Context;
use plane_core::config::{Role, Settings};
use plane_db::DbPools;

fn main() -> anyhow::Result<()> {
    let settings =
        Settings::from_env(Role::Migrator).map_err(|error| anyhow::anyhow!("{error}"))?;
    let db = DbPools::from_settings(&settings)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let mut client = db
            .write_pool()
            .get()
            .await
            .context("connecting to Postgres")?;
        match schema::migrate(&mut client).await.context("migrating")? {
            schema::Outcome::FreshInstall { migrations } => {
                println!(
                    "plane-migrator: fresh install: applied the Django baseline \
                     ({migrations} migrations) and created the plane_jobs schema"
                );
            }
            schema::Outcome::Upgrade { migrations } => {
                println!(
                    "plane-migrator: baseline matched ({migrations} migrations); \
                     plane_jobs schema is up to date"
                );
            }
            schema::Outcome::AlreadyCurrent => {
                println!("plane-migrator: already up to date");
            }
        }
        Ok(())
    })
}
