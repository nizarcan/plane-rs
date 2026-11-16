// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Celery coexistence publisher and typed task API.
//!
//! Start with [`CeleryPublisher::from_settings`] (or [`CeleryPublisher::from_url`]
//! in tests), then call one of the typed functions in [`tasks`] and hand the
//! message to [`CeleryPublisher::publish`]:
//!
//! ```no_run
//! use plane_jobs::celery::publisher::CeleryPublisher;
//! use plane_jobs::celery::tasks::{RecentVisitedTask, recent_visited_task};
//! use plane_jobs::celery::args::TaskArg;
//!
//! # async fn example(publisher: &CeleryPublisher, user: uuid::Uuid, project: uuid::Uuid) {
//! let message = recent_visited_task(RecentVisitedTask::ProjectKeyed {
//!     slug: "acme".to_string(),
//!     project_id: Some(TaskArg::uuid(project)),
//!     entity_name: "project".to_string(),
//!     entity_identifier: TaskArg::uuid(project),
//!     user_id: TaskArg::uuid(user),
//! });
//! let task_id = publisher.publish(&message).await.expect("broker reachable");
//! # let _ = task_id;
//! # }
//! ```
//!
//! Modules:
//!
//! * [`args`] — [`TaskArg`] and the `json.dumps(…, cls=DjangoJSONEncoder)`
//!   helper.
//! * [`tasks`] — one typed constructor per task, with the per-call-site
//!   argument types.
//! * [`message`] — the protocol-2 message and its exact rendering.
//! * [`publisher`] — the AMQP transport.
//! * [`broker_url`] — kombu's URL semantics.
//! * [`repr`] — Celery's `argsrepr`/`kwargsrepr` (not Python's `repr()`).

pub mod args;
pub mod broker_url;
pub mod message;
pub mod publisher;
pub mod repr;
pub mod tasks;

#[cfg(any(test, feature = "test-cases"))]
pub mod test_cases;

#[cfg(test)]
mod tests;

pub use args::{EncodeError, TaskArg};
pub use broker_url::{BrokerUrlError, CeleryConfig};
pub use message::{
    DEFAULT_EXCHANGE, DEFAULT_QUEUE, DEFAULT_ROUTING_KEY, EncodeContext, EncodedMessage, Headers,
    Message, Properties, task_name,
};
pub use publisher::{CeleryPublisher, PublishError};
