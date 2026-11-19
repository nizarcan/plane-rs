// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Python containers' boot steps, as reusable Rust functions
//! (`plans/CUTOVER.md` §3).
//!
//! `apps/api/bin/docker-entrypoint-api.sh` runs, in order:
//!
//! ```text
//! wait_for_db → wait_for_migrations → register_instance <sig> →
//! configure_instance → create_bucket → clear_cache → collectstatic
//! ```
//!
//! and the worker/beat entrypoints run the first two. In Rust the migrator is
//! the only process that migrates (the compose `migrator` service is a
//! `service_completed_successfully` dependency), so every binary waits for its
//! marker instead of racing it, and `/static/` is served from embedded bytes
//! rather than `collectstatic`.
//!
//! * [`wait`] — the connect retry and the migrations-ready check.
//! * [`instance`] — `register_instance` (created/extended `Instance`, GitHub
//!   `releases/latest`) and `configure_instance` (the 36
//!   `InstanceConfiguration` rows, Fernet-encrypted where Django encrypts).
//! * [`bucket`] — `create_bucket` (head/create, errors swallowed and logged).
//! * [`cache`] — `clear_cache` (`FLUSHDB`, as `django_redis`'s `client.clear()`).
//! * [`static_files`] — the eight files of `apps/api/plane/static`, embedded.
//! * [`admin`] — `plane-admin`'s `reset-password`, `activate-user` and
//!   `test-email`, ported from `plane/db/management/commands`.
//!
//! The database effects are proven against the Django commands on cloned
//! databases by `tools/boot/compare_boot.py`.

pub mod admin;
pub mod bucket;
pub mod cache;
pub mod config_variables;
pub mod instance;
pub mod static_files;
pub mod wait;

/// Errors the boot steps report to their binary. A boot step that Django's
/// command lets fail (`CommandError`, an uncaught exception) fails startup
/// here; the steps that swallow errors say so in their own documentation.
#[derive(Debug, thiserror::Error)]
pub enum BootError {
    #[error(transparent)]
    Database(#[from] plane_db::DbError),
    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),
    #[error(transparent)]
    Redis(#[from] plane_cache::Error),
    #[error(transparent)]
    Storage(#[from] plane_storage::StorageError),
    #[error(transparent)]
    Mail(#[from] plane_mail::MailError),
    #[error(transparent)]
    Template(#[from] plane_mail::TemplateError),
    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, BootError>;
