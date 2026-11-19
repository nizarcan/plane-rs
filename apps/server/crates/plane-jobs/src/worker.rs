// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The worker: claim with `SKIP LOCKED`, run a Rust handler, retry or dead-letter.
//!
//! At-least-once is deliberate (a lease expires after a crash); handlers must
//! be idempotent or guarded. A handler that returns [`HandlerError::fatal`] is
//! dead-lettered immediately; a retryable error follows the job's retry policy.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use deadpool_postgres::Client;
use plane_db::DbPools;
use serde_json::Value;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::listener::Listener;
use crate::queue::{self, ClaimedJob, Failure, QueueError};

/// A handler failure and whether another attempt can help.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct HandlerError {
    pub message: String,
    pub retryable: bool,
}

impl HandlerError {
    #[must_use]
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }

    #[must_use]
    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

/// What a handler receives: its own connection, the job row and the payload.
pub struct JobContext {
    pub client: Client,
    pub id: Uuid,
    pub task: String,
    pub payload: Value,
    pub attempts: i32,
}

pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send>>;
type Handler = Arc<dyn Fn(JobContext) -> HandlerFuture + Send + Sync>;

/// The Rust-owned task handlers. Registering a task is the other half of
/// moving it in `ownership::RUST_TASKS`: a row with no handler is dead.
#[derive(Default)]
pub struct TaskRegistry {
    handlers: HashMap<String, Handler>,
}

impl TaskRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<F, Fut>(&mut self, task: impl Into<String>, handler: F)
    where
        F: Fn(JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.handlers.insert(
            task.into(),
            Arc::new(move |ctx| Box::pin(handler(ctx)) as HandlerFuture),
        );
    }

    fn get(&self, task: &str) -> Option<Handler> {
        self.handlers.get(task).cloned()
    }

    #[must_use]
    pub fn contains(&self, task: &str) -> bool {
        self.handlers.contains_key(task)
    }
}

/// Worker knobs. The defaults are the Phase 4 configuration documented in
/// `ENV.md`; the binaries override them from the environment.
#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub concurrency: usize,
    pub lease: Duration,
    pub poll: Duration,
    pub drain: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            lease: Duration::from_secs(60),
            poll: Duration::from_secs(1),
            drain: Duration::from_secs(30),
        }
    }
}

/// One worker identity (`host:pid:uuid`). It owns the lease; a second worker
/// with the same id would be able to overwrite the first, so the id includes a
/// per-process uuid.
pub struct Worker {
    pool: DbPools,
    registry: Arc<TaskRegistry>,
    config: WorkerConfig,
    worker_id: String,
}

impl Worker {
    #[must_use]
    pub fn new(
        pool: DbPools,
        registry: TaskRegistry,
        config: WorkerConfig,
        worker_id: String,
    ) -> Self {
        Self {
            pool,
            registry: Arc::new(registry),
            config,
            worker_id,
        }
    }

    #[must_use]
    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    /// Run until `shutdown` resolves, then drain in-flight handlers and release
    /// anything still `running` for this worker.
    pub async fn run<S>(self, shutdown: S, mut listener: Option<Listener>) -> Result<(), QueueError>
    where
        S: Future<Output = ()>,
    {
        tokio::pin!(shutdown);
        let mut inflight: JoinSet<()> = JoinSet::new();
        let mut stopping = false;

        loop {
            while let Some(result) = inflight.try_join_next() {
                if let Err(error) = result {
                    tracing::error!(target: "plane.jobs", %error, "job task panicked");
                }
            }

            if stopping {
                break;
            }

            let available = self.config.concurrency.saturating_sub(inflight.len());
            if available > 0 {
                let client = self.pool.write_pool().get().await?;
                let jobs = queue::claim(
                    &**client,
                    &self.worker_id,
                    available as i64,
                    self.config.lease.as_secs() as i64,
                )
                .await?;
                drop(client);
                if !jobs.is_empty() {
                    for job in jobs {
                        self.spawn(&mut inflight, job);
                    }
                    continue;
                }
            }

            if inflight.is_empty() {
                tokio::select! {
                    () = &mut shutdown => { stopping = true; }
                    () = tokio::time::sleep(self.config.poll) => {}
                    () = wait_for_notification(&mut listener) => {}
                }
            } else {
                tokio::select! {
                    () = &mut shutdown => { stopping = true; }
                    Some(_) = inflight.join_next() => {}
                }
            }
        }

        let drained = tokio::time::timeout(self.config.drain, async {
            while inflight.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            tracing::warn!(
                target: "plane.jobs",
                worker = %self.worker_id,
                "drain deadline reached; aborting in-flight handlers"
            );
            inflight.abort_all();
            while inflight.join_next().await.is_some() {}
        }

        // A handler may have panicked or been aborted; those rows are still
        // `running` under this worker. Releasing is idempotent for the rest.
        let client = self.pool.write_pool().get().await?;
        let released = queue::release(&**client, &self.worker_id).await?;
        if released > 0 {
            tracing::info!(
                target: "plane.jobs",
                worker = %self.worker_id,
                released,
                "released jobs after shutdown"
            );
        }
        Ok(())
    }

    /// Claim and process until the queue has no runnable job for this worker.
    /// Used by tests and by a future `plane-worker --once`.
    pub async fn drain(&self) -> Result<usize, QueueError> {
        self.drain_limited(usize::MAX).await
    }

    /// [`drain`] with a cap on the jobs executed. `difftest task` caps it at
    /// one so a handler's published tasks stay queued, exactly as they do when
    /// Django's `task.apply()` publishes them (`D25`).
    pub async fn drain_limited(&self, limit: usize) -> Result<usize, QueueError> {
        let mut processed = 0;
        while processed < limit {
            let client = self.pool.write_pool().get().await?;
            let jobs = queue::claim(
                &**client,
                &self.worker_id,
                1,
                self.config.lease.as_secs() as i64,
            )
            .await?;
            drop(client);
            if jobs.is_empty() {
                return Ok(processed);
            }
            for job in jobs {
                self.process_one(job).await?;
                processed += 1;
            }
        }
        Ok(processed)
    }

    fn spawn(&self, inflight: &mut JoinSet<()>, job: ClaimedJob) {
        let pool = self.pool.write_pool().clone();
        let registry = Arc::clone(&self.registry);
        let worker_id = self.worker_id.clone();
        inflight.spawn(async move {
            if let Err(error) = process(pool, registry, &worker_id, job).await {
                tracing::error!(target: "plane.jobs", %error, "processing a claimed job failed");
            }
        });
    }

    async fn process_one(&self, job: ClaimedJob) -> Result<(), QueueError> {
        process(
            self.pool.write_pool().clone(),
            Arc::clone(&self.registry),
            &self.worker_id,
            job,
        )
        .await
    }
}

async fn wait_for_notification(listener: &mut Option<Listener>) {
    match listener {
        Some(listener) => {
            listener.recv().await;
        }
        None => std::future::pending::<()>().await,
    }
}

async fn process(
    pool: deadpool_postgres::Pool,
    registry: Arc<TaskRegistry>,
    worker_id: &str,
    job: ClaimedJob,
) -> Result<(), QueueError> {
    let Some(handler) = registry.get(&job.task) else {
        let client = pool.get().await?;
        queue::fail(
            &**client,
            worker_id,
            &job,
            &format!("no handler registered for task {}", job.task),
            false,
        )
        .await?;
        return Ok(());
    };

    let context = JobContext {
        client: pool.get().await?,
        id: job.id,
        task: job.task.clone(),
        payload: job.payload.clone(),
        attempts: job.attempts,
    };
    let outcome = handler(context).await;

    let client = pool.get().await?;
    match outcome {
        Ok(()) => {
            queue::complete(&**client, worker_id, job.id).await?;
        }
        Err(error) => {
            match queue::fail(&**client, worker_id, &job, &error.message, error.retryable).await? {
                Failure::Retry { run_at } => tracing::warn!(
                    target: "plane.jobs",
                    task = %job.task,
                    attempts = job.attempts,
                    %run_at,
                    "job failed; retry scheduled"
                ),
                Failure::Dead => tracing::error!(
                    target: "plane.jobs",
                    task = %job.task,
                    attempts = job.attempts,
                    "job failed; dead-lettered"
                ),
            }
        }
    }
    Ok(())
}
