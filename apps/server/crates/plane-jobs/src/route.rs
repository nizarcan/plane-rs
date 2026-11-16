// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The one call-site API: route a job to its owner.
//!
//! A handler builds both representations — the typed Celery message (for the
//! byte-identical coexistence wire format) and the Rust [`Job`] — then calls
//! [`dispatch`]. The ownership table decides which one leaves the process.
//! When the task is Celery-owned, no DB connection is touched.

use tokio_postgres::GenericClient;
use uuid::Uuid;

use crate::ownership::{Owner, owner};
use crate::queue::{self, Job, QueueError};

/// Where the job went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Published to RabbitMQ; carries Celery's task id.
    Celery(Uuid),
    /// Inserted into `plane_jobs.jobs`; `None` means a dedupe key suppressed it.
    Queued(Option<Uuid>),
}

#[cfg(feature = "celery")]
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error(transparent)]
    Queue(#[from] QueueError),
    #[error(transparent)]
    Celery(#[from] crate::celery::PublishError),
    #[error("task {0} is Celery-owned but this process has no publisher")]
    NoPublisher(String),
    #[error("task {0} is Rust-owned but no database connection was supplied")]
    NoDatabase(String),
}

/// Route one task according to [`owner`].
///
/// The Celery message is required in both branches because a caller cannot
/// know at compile time which side owns the task after an operator changes the
/// table; the Rust `Job` is only inserted for the Rust branch.
#[cfg(feature = "celery")]
pub async fn dispatch<C: GenericClient + Sync>(
    task: &str,
    job: &Job,
    message: &crate::celery::Message,
    publisher: Option<&crate::celery::CeleryPublisher>,
    client: Option<&C>,
) -> Result<Delivery, DispatchError> {
    match owner(task) {
        Owner::Celery => {
            let publisher = publisher.ok_or_else(|| DispatchError::NoPublisher(task.to_owned()))?;
            Ok(Delivery::Celery(publisher.publish(message).await?))
        }
        Owner::Rust => {
            let client = client.ok_or_else(|| DispatchError::NoDatabase(task.to_owned()))?;
            Ok(Delivery::Queued(queue::enqueue(client, job).await?))
        }
    }
}
