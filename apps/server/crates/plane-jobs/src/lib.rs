// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Background job plumbing for the Rust rewrite.
//!
//! This crate has two lives:
//!
//! * During coexistence (Phase 4, `RUST_REWRITE.md` "Background jobs") a Rust
//!   handler that needs a task still owned by Celery publishes a
//!   Celery-protocol JSON message to RabbitMQ. [`celery`] is that publisher and
//!   the typed task API that goes with it.
//! * After cutover the crate owns a Postgres-backed queue
//!   (`SELECT … FOR UPDATE SKIP LOCKED` in schema `plane_jobs`, DECISIONS.md
//!   D8). That part does not exist yet.
//!
//! The Celery half is behind the `celery` cargo feature (D7), which is on by
//! default while Django still runs the workers.

#[cfg(feature = "celery")]
pub mod celery;
