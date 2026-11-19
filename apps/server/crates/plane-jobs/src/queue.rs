// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Postgres queue: schema, enqueue, claim, complete and retry.
//!
//! Every statement here is raw, fixed SQL, so `prepare_cached` is used on the
//! request path in `plane-api`; the worker's long-running statements are
//! delegated to `tokio-postgres` without a per-connection cache because the
//! worker owns its connections for its whole lifetime.
//!
//! The schema lives in its own Postgres schema (`plane_jobs`), never in
//! Django's `public` schema. `migrate` is idempotent and is the only code that
//! changes it.

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio_postgres::GenericClient;
use uuid::Uuid;

/// The notify channel `enqueue` raises and the worker listens on.
pub const NOTIFY_CHANNEL: &str = "plane_jobs_jobs";

const MIGRATION: &str = r#"
CREATE SCHEMA IF NOT EXISTS plane_jobs;

CREATE TABLE IF NOT EXISTS plane_jobs.jobs (
    id uuid PRIMARY KEY,
    task text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    state text NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'running', 'done', 'failed', 'dead')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_retries integer NOT NULL DEFAULT 0 CHECK (max_retries >= 0),
    retry_backoff_seconds integer NOT NULL DEFAULT 0
        CHECK (retry_backoff_seconds >= 0),
    retry_jitter boolean NOT NULL DEFAULT false,
    run_at timestamptz NOT NULL DEFAULT now(),
    lease_until timestamptz,
    locked_by text,
    last_error text,
    dedupe_key text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz
);

CREATE INDEX IF NOT EXISTS jobs_ready_idx
    ON plane_jobs.jobs (run_at, id)
    WHERE state IN ('queued', 'failed');

CREATE INDEX IF NOT EXISTS jobs_lease_idx
    ON plane_jobs.jobs (lease_until)
    WHERE state = 'running';

CREATE UNIQUE INDEX IF NOT EXISTS jobs_dedupe_idx
    ON plane_jobs.jobs (dedupe_key)
    WHERE dedupe_key IS NOT NULL AND state IN ('queued', 'running');

CREATE TABLE IF NOT EXISTS plane_jobs.schedules (
    name text PRIMARY KEY,
    task text NOT NULL,
    owner text NOT NULL DEFAULT 'celery' CHECK (owner IN ('celery', 'rust')),
    kind text NOT NULL CHECK (kind IN ('cron', 'interval')),
    cron text,
    interval_seconds bigint,
    enabled boolean NOT NULL DEFAULT true,
    last_run_at timestamptz,
    handover_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE plane_jobs.schedules
    ADD COLUMN IF NOT EXISTS handover_at timestamptz;

CREATE TABLE IF NOT EXISTS plane_jobs.ticks (
    schedule_name text NOT NULL,
    tick_at timestamptz NOT NULL,
    fired_at timestamptz NOT NULL DEFAULT now(),
    job_id uuid,
    PRIMARY KEY (schedule_name, tick_at)
);

CREATE TABLE IF NOT EXISTS plane_jobs.meta (
    key text PRIMARY KEY,
    value text NOT NULL
);
"#;

/// The queue's error type. `NoDatabase` is the handler path's explicit refusal
/// rather than silently dropping a job.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),
    #[error(transparent)]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error(transparent)]
    Database(#[from] plane_db::DbError),
    #[error(transparent)]
    Url(#[from] plane_db::url::DatabaseUrlError),
    #[error("no database connection is available for the job queue")]
    NoDatabase,
}

/// A job that should run once. `max_retries = 0` means dead on first failure,
/// matching the explicit policy this queue requires (Python swallowed most
/// task errors and Celery reported success).
#[derive(Clone, Debug)]
pub struct Job {
    pub task: String,
    pub payload: Value,
    pub dedupe_key: Option<String>,
    pub run_at: Option<DateTime<Utc>>,
    pub max_retries: i32,
    pub retry_backoff_seconds: i32,
    pub retry_jitter: bool,
}

impl Job {
    /// A job with no retries and an empty JSON payload.
    #[must_use]
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            payload: Value::Object(serde_json::Map::new()),
            dedupe_key: None,
            run_at: None,
            max_retries: 0,
            retry_backoff_seconds: 0,
            retry_jitter: false,
        }
    }

    #[must_use]
    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }

    #[must_use]
    pub fn dedupe_key(mut self, key: impl Into<String>) -> Self {
        self.dedupe_key = Some(key.into());
        self
    }

    #[must_use]
    pub fn run_at(mut self, at: DateTime<Utc>) -> Self {
        self.run_at = Some(at);
        self
    }

    /// `max_retries` retries after the first attempt; `backoff` seconds before
    /// each retry, jittered uniformly over `[0, backoff]` when requested.
    #[must_use]
    pub fn retry(mut self, max_retries: i32, backoff: i32, jitter: bool) -> Self {
        self.max_retries = max_retries.max(0);
        self.retry_backoff_seconds = backoff.max(0);
        self.retry_jitter = jitter;
        self
    }

    /// The Rust-owned representation of a typed Celery message.
    ///
    /// The payload is `{"args": [...], "kwargs": {...}}` with Python runtime
    /// values (a `uuid.UUID` is its string, `json.dumps` text stays a string),
    /// so a switch from `.delay()` to the Postgres queue is transport-only and
    /// a handler can be tested against both representations
    /// (`plans/CUTOVER.md` §2 "Payload equivalence").
    ///
    /// The task's retry policy is applied from [`crate::policy`]: neither the
    /// Celery message nor the call site carries it.
    #[cfg(feature = "celery")]
    #[must_use]
    pub fn from_message(message: &crate::celery::Message) -> Self {
        let args: Vec<serde_json::Value> = message
            .args()
            .iter()
            .map(crate::celery::args::TaskArg::python_value)
            .collect();
        let kwargs: serde_json::Map<String, serde_json::Value> = message
            .kwargs()
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.python_value()))
            .collect();
        let mut job = Self::new(message.task()).payload(serde_json::json!({
            "args": args,
            "kwargs": kwargs,
        }));
        let policy = crate::policy::policy(message.task());
        job.max_retries = policy.max_retries;
        job.retry_backoff_seconds = policy.retry_backoff_seconds;
        job.retry_jitter = policy.retry_jitter;
        // `countdown=` / an `eta` published by the call site becomes the job's
        // due instant (`plans/CUTOVER.md` §2: `countdown` → `Job.run_at`). The
        // Celery message resolves the same countdown against its own publish
        // instant, a few microseconds earlier.
        if let Some(seconds) = message.countdown() {
            job.run_at = Some(chrono::Utc::now() + chrono::TimeDelta::seconds(seconds));
        }
        job
    }
}

/// A claimed row. `attempts` includes the attempt that is about to run.
#[derive(Clone, Debug)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub task: String,
    pub payload: Value,
    pub attempts: i32,
    pub max_retries: i32,
    pub retry_backoff_seconds: i32,
    pub retry_jitter: bool,
}

/// The outcome of a failed attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Retry is pending at the included time.
    Retry { run_at: DateTime<Utc> },
    /// The row is now `dead`.
    Dead,
}

/// Create the `plane_jobs` schema, tables and indexes. Safe to run every time
/// a process starts; it never touches a Django table.
pub async fn migrate<C: GenericClient + Sync>(client: &C) -> Result<(), QueueError> {
    client.batch_execute(MIGRATION).await?;
    client
        .execute(
            "INSERT INTO plane_jobs.meta (key, value) VALUES ('schema_version', '2') \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            &[],
        )
        .await?;
    Ok(())
}

/// Insert one job, sending `NOTIFY` in the same transaction when the caller
/// passes a transaction. Returns `None` when a dedupe key is already queued or
/// running (the partial unique index decides, so two concurrent enqueues cannot
/// both win).
pub async fn enqueue<C: GenericClient + Sync>(
    client: &C,
    job: &Job,
) -> Result<Option<Uuid>, QueueError> {
    let id = Uuid::new_v4();
    let row = client
        .query_opt(
            "INSERT INTO plane_jobs.jobs \
             (id, task, payload, dedupe_key, run_at, max_retries, \
              retry_backoff_seconds, retry_jitter) \
             VALUES ($1, $2, $3, $4, COALESCE($5, now()), $6, $7, $8) \
             ON CONFLICT DO NOTHING \
             RETURNING id",
            &[
                &id,
                &job.task,
                &job.payload,
                &job.dedupe_key,
                &job.run_at,
                &job.max_retries,
                &job.retry_backoff_seconds,
                &job.retry_jitter,
            ],
        )
        .await?;
    if row.is_none() {
        return Ok(None);
    }
    client
        .execute(
            "SELECT pg_notify('plane_jobs_jobs', $1)",
            &[&id.to_string()],
        )
        .await?;
    Ok(Some(id))
}

/// Claim up to `limit` due or lease-expired jobs for `worker_id`.
///
/// One statement: the `FOR UPDATE SKIP LOCKED` CTE picks rows, the `UPDATE`
/// takes them and returns them. Four concurrent workers cannot see the same
/// row.
pub async fn claim<C: GenericClient + Sync>(
    client: &C,
    worker_id: &str,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<ClaimedJob>, QueueError> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "WITH picked AS ( \
                 SELECT id FROM plane_jobs.jobs \
                 WHERE (state IN ('queued', 'failed') AND run_at <= now()) \
                    OR (state = 'running' AND lease_until < now()) \
                 ORDER BY run_at, id \
                 FOR UPDATE SKIP LOCKED \
                 LIMIT $3 \
             ) \
             UPDATE plane_jobs.jobs j \
             SET state = 'running', attempts = j.attempts + 1, locked_by = $1, \
                 lease_until = now() + ($2::bigint * interval '1 second'), \
                 updated_at = now() \
             FROM picked p \
             WHERE j.id = p.id \
             RETURNING j.id, j.task, j.payload, j.attempts, j.max_retries, \
                       j.retry_backoff_seconds, j.retry_jitter",
            &[&worker_id, &lease_seconds, &limit],
        )
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ClaimedJob {
                id: row.try_get(0)?,
                task: row.try_get(1)?,
                payload: row.try_get(2)?,
                attempts: row.try_get(3)?,
                max_retries: row.try_get(4)?,
                retry_backoff_seconds: row.try_get(5)?,
                retry_jitter: row.try_get(6)?,
            })
        })
        .collect()
}

/// Mark a claimed job done. The `locked_by` guard means a worker that lost its
/// lease cannot overwrite a newer owner.
pub async fn complete<C: GenericClient + Sync>(
    client: &C,
    worker_id: &str,
    id: Uuid,
) -> Result<bool, QueueError> {
    Ok(client
        .execute(
            "UPDATE plane_jobs.jobs \
             SET state = 'done', finished_at = now(), locked_by = NULL, \
                 lease_until = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running' AND locked_by = $2",
            &[&id, &worker_id],
        )
        .await?
        > 0)
}

/// Record a failed attempt. Retryable failures with budget left become
/// `failed` (runnable at the returned time); exhaustion or a non-retryable
/// error becomes `dead`.
pub async fn fail<C: GenericClient + Sync>(
    client: &C,
    worker_id: &str,
    job: &ClaimedJob,
    error: &str,
    retryable: bool,
) -> Result<Failure, QueueError> {
    let exhausted = !retryable || job.attempts > job.max_retries;
    if exhausted {
        client
            .execute(
                "UPDATE plane_jobs.jobs \
                 SET state = 'dead', last_error = $3, finished_at = now(), \
                     locked_by = NULL, lease_until = NULL, updated_at = now() \
                 WHERE id = $1 AND state = 'running' AND locked_by = $2",
                &[&job.id, &worker_id, &error],
            )
            .await?;
        return Ok(Failure::Dead);
    }

    let seconds = if job.retry_jitter && job.retry_backoff_seconds > 0 {
        (job.id.as_u128() % (job.retry_backoff_seconds as u128 + 1)) as i64
    } else {
        i64::from(job.retry_backoff_seconds)
    };
    let row = client
        .query_one(
            "UPDATE plane_jobs.jobs \
             SET state = 'failed', last_error = $3, \
                 run_at = now() + ($4::bigint * interval '1 second'), \
                 locked_by = NULL, lease_until = NULL, updated_at = now() \
             WHERE id = $1 AND state = 'running' AND locked_by = $2 \
             RETURNING run_at",
            &[&job.id, &worker_id, &error, &seconds],
        )
        .await?
        .try_get::<_, DateTime<Utc>>(0)?;
    Ok(Failure::Retry { run_at: row })
}

/// Requeue every `running` row owned by `worker_id` during a graceful
/// shutdown. A shutdown release does not consume a retry, so decrement the
/// attempt the worker had just started.
pub async fn release<C: GenericClient + Sync>(
    client: &C,
    worker_id: &str,
) -> Result<u64, QueueError> {
    Ok(client
        .execute(
            "UPDATE plane_jobs.jobs \
             SET state = 'queued', run_at = now(), \
                 attempts = GREATEST(attempts - 1, 0), \
                 locked_by = NULL, lease_until = NULL, updated_at = now() \
             WHERE locked_by = $1 AND state = 'running'",
            &[&worker_id],
        )
        .await?)
}

/// `pg_try_advisory_xact_lock` for one schedule tick. The transaction-scoped
/// lock is released on commit/rollback; a second beat replica returns false.
pub async fn try_tick_lock<C: GenericClient + Sync>(
    client: &C,
    schedule: &str,
    tick: DateTime<Utc>,
) -> Result<bool, QueueError> {
    let key = format!("{schedule}:{}", tick.timestamp());
    Ok(client
        .query_one(
            "SELECT pg_try_advisory_xact_lock(hashtextextended($1, 0))",
            &[&key],
        )
        .await?
        .try_get(0)?)
}

/// Record a tick's exactly-once claim. Returns false when another beat (or a
/// previous process) already fired it.
pub async fn claim_tick<C: GenericClient + Sync>(
    client: &C,
    schedule: &str,
    tick: DateTime<Utc>,
    job_id: Uuid,
) -> Result<bool, QueueError> {
    Ok(client
        .query_opt(
            "INSERT INTO plane_jobs.ticks (schedule_name, tick_at, job_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (schedule_name, tick_at) DO NOTHING \
             RETURNING tick_at",
            &[&schedule, &tick, &job_id],
        )
        .await?
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_no_retry_and_no_dedupe() {
        let job = Job::new("plane.jobs.echo");
        assert_eq!(job.task, "plane.jobs.echo");
        assert_eq!(job.max_retries, 0);
        assert_eq!(job.retry_backoff_seconds, 0);
        assert!(!job.retry_jitter);
        assert!(job.dedupe_key.is_none());
        assert!(job.run_at.is_none());
    }

    #[test]
    fn retry_clamps_negative_values() {
        let job = Job::new("x").retry(-4, -2, true);
        assert_eq!(job.max_retries, 0);
        assert_eq!(job.retry_backoff_seconds, 0);
        assert!(job.retry_jitter);
    }

    /// `plans/CUTOVER.md` §2(b): the queue payload is the Celery message's
    /// `(args, kwargs)` with Python runtime values, so switching the transport
    /// cannot change what the handler sees.
    #[cfg(feature = "celery")]
    #[test]
    fn from_message_keeps_python_values_and_the_task_policy() {
        use crate::celery::args::TaskArg;
        use crate::celery::tasks::{RecentVisitedTask, recent_visited_task};

        let user = Uuid::nil();
        let project = Uuid::max();
        let message = recent_visited_task(RecentVisitedTask::ProjectKeyed {
            slug: "acme".to_string(),
            project_id: Some(TaskArg::uuid(project)),
            entity_name: "project".to_string(),
            entity_identifier: TaskArg::uuid(user),
            user_id: TaskArg::uuid(user),
        });
        let job = Job::from_message(&message);
        assert_eq!(job.task, message.task());
        assert_eq!(
            job.payload,
            serde_json::json!({
                "args": [],
                "kwargs": {
                    "slug": "acme",
                    "project_id": project.hyphenated().to_string(),
                    "entity_name": "project",
                    "entity_identifier": user.hyphenated().to_string(),
                    "user_id": user.hyphenated().to_string(),
                },
            })
        );
        assert_eq!(job.max_retries, 0);
        assert_eq!(job.retry_backoff_seconds, 0);
        assert!(!job.retry_jitter);
    }
}
