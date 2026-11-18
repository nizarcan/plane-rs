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
//! * The Postgres queue (`SELECT … FOR UPDATE SKIP LOCKED` in schema
//!   `plane_jobs`, DECISIONS.md D8) is [`queue`], [`worker`] and [`beat`].
//!   [`ownership`] decides which side owns a task; [`route::dispatch`] is the
//!   one call site a handler uses.
//!
//! The Celery half is behind the `celery` cargo feature (D7), which is on by
//! default while Django still runs the workers. The queue half has no such
//! feature: it is the cutover target.

#[cfg(feature = "celery")]
pub mod celery;

pub mod beat;
pub mod listener;
pub mod ownership;
pub mod policy;
pub mod queue;
#[cfg(feature = "celery")]
pub mod route;
pub mod worker;

pub use beat::{Beat, BeatConfig, REAL_SCHEDULES, ScheduleSeed};
pub use ownership::{Owner, owner};
pub use policy::{Producer, TaskPolicy, policy as task_policy};
pub use queue::{ClaimedJob, Failure, Job, QueueError, enqueue, migrate};
pub use worker::{HandlerError, JobContext, TaskRegistry, Worker, WorkerConfig};
