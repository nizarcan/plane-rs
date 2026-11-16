// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-migrator`: create the Rust-owned `plane_jobs` schema.
//!
//! It is deliberately separate from Django's `manage.py migrate`: the queue is
//! the one schema addition and it must not appear in
//! `apps/api/migrations_lockfile.txt`.

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
        let client = db
            .write_pool()
            .get()
            .await
            .context("connecting to Postgres")?;
        plane_jobs::migrate(&**client)
            .await
            .context("creating the plane_jobs schema")?;
        println!("plane-migrator: plane_jobs schema is up to date");
        Ok(())
    })
}
