// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DB-backed queue, worker and beat tests.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_rsjobs_test
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_rsjobs_test \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-jobs --test jobs
//! ```
//!
//! The schema is dropped and recreated per test; the tests share one database,
//! so a process-wide async mutex serialises them.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use plane_core::config::{EnvMap, Role, Settings};
use plane_db::DbPools;
use plane_jobs::beat::{Beat, BeatConfig};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, RecentVisitedTask};
use plane_jobs::queue::{self, Failure, Job};
use plane_jobs::route::{self, Delivery};
use plane_jobs::worker::{JobContext, TaskRegistry, Worker, WorkerConfig};
use tokio_postgres::NoTls;

static SERIAL: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn database_url() -> Option<String> {
    let url = std::env::var("PLANE_RS_TEST_DATABASE_URL").ok()?;
    if url.is_empty() { None } else { Some(url) }
}

async fn connect() -> Option<tokio_postgres::Client> {
    let url = match database_url() {
        Some(url) => url,
        None => {
            if std::env::var("PLANE_RS_REQUIRE_DB_TESTS").as_deref() == Ok("1") {
                panic!("PLANE_RS_REQUIRE_DB_TESTS=1 but PLANE_RS_TEST_DATABASE_URL is unset");
            }
            eprintln!("jobs tests: PLANE_RS_TEST_DATABASE_URL is unset; skipping");
            return None;
        }
    };
    let (client, connection) = tokio_postgres::connect(&url, NoTls)
        .await
        .expect("connect to the test database");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Some(client)
}

async fn reset(client: &tokio_postgres::Client) {
    client
        .batch_execute("DROP SCHEMA IF EXISTS plane_jobs CASCADE")
        .await
        .expect("drop schema");
    plane_jobs::migrate(client).await.expect("migrate");
    // migrate is idempotent
    plane_jobs::migrate(client).await.expect("migrate twice");
}

fn settings(url: &str) -> Settings {
    Settings::from_source(
        &EnvMap::new()
            .set("DATABASE_URL", url)
            .set("REDIS_URL", "redis://127.0.0.1:56379/5")
            .set("SECRET_KEY", "jobs-test"),
        Role::Worker,
    )
    .expect("test settings")
}

fn registry(sleep_ms: u64) -> TaskRegistry {
    let mut registry = TaskRegistry::new();
    registry.register("plane.jobs.echo", move |_ctx: JobContext| async move {
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        Ok(())
    });
    registry
}

async fn enqueue_many(client: &tokio_postgres::Client, count: usize) {
    for index in 0..count {
        let job = Job::new("plane.jobs.echo").payload(serde_json::json!({ "n": index }));
        queue::enqueue(client, &job).await.expect("enqueue");
    }
}

async fn count(client: &tokio_postgres::Client, sql: &str) -> i64 {
    client
        .query_one(sql, &[])
        .await
        .expect("count query")
        .get(0)
}

#[tokio::test]
async fn enqueue_inside_a_rolled_back_transaction_leaves_no_job() {
    let Some(mut client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;

    let tx = client.transaction().await.expect("transaction");
    let id = queue::enqueue(&tx, &Job::new("plane.jobs.echo"))
        .await
        .expect("enqueue in transaction")
        .expect("inserted");
    let inside: i64 = tx
        .query_one("SELECT count(*) FROM plane_jobs.jobs WHERE id = $1", &[&id])
        .await
        .expect("count in transaction")
        .get(0);
    assert_eq!(inside, 1);
    tx.rollback().await.expect("rollback");

    assert_eq!(
        count(&client, "SELECT count(*) FROM plane_jobs.jobs").await,
        0
    );
}

#[tokio::test]
async fn four_workers_claim_one_thousand_jobs_exactly_once() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;
    enqueue_many(&client, 1_000).await;

    let url = database_url().expect("url");
    let processed = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for worker in 0..4 {
        let url = url.clone();
        let processed = Arc::clone(&processed);
        tasks.push(tokio::spawn(async move {
            let (client, connection) = tokio_postgres::connect(&url, NoTls).await.expect("connect");
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let worker_id = format!("test-worker-{worker}");
            loop {
                let jobs = queue::claim(&client, &worker_id, 1, 30)
                    .await
                    .expect("claim");
                if jobs.is_empty() {
                    return;
                }
                for job in jobs {
                    processed.fetch_add(1, Ordering::SeqCst);
                    assert!(
                        queue::complete(&client, &worker_id, job.id)
                            .await
                            .expect("complete")
                    );
                }
            }
        }));
    }
    for task in tasks {
        task.await.expect("worker task");
    }

    assert_eq!(processed.load(Ordering::SeqCst), 1_000);
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM plane_jobs.jobs WHERE state = 'done'"
        )
        .await,
        1_000
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM plane_jobs.jobs WHERE attempts <> 1"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn an_expired_lease_is_retaken_and_counts_an_attempt() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;

    let id = queue::enqueue(&client, &Job::new("plane.jobs.echo"))
        .await
        .expect("enqueue")
        .expect("inserted");
    let first = queue::claim(&client, "worker-a", 1, 60)
        .await
        .expect("claim");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, id);
    assert_eq!(first[0].attempts, 1);
    assert!(
        queue::claim(&client, "worker-b", 1, 60)
            .await
            .expect("second claim")
            .is_empty()
    );

    client
        .execute(
            "UPDATE plane_jobs.jobs SET lease_until = now() - interval '1 second' WHERE id = $1",
            &[&id],
        )
        .await
        .expect("expire lease");
    let second = queue::claim(&client, "worker-b", 1, 60)
        .await
        .expect("retake");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].id, id);
    assert_eq!(second[0].attempts, 2);
    assert!(
        queue::complete(&client, "worker-b", id)
            .await
            .expect("complete")
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM plane_jobs.jobs WHERE state = 'done'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_retryable_failure_retries_then_dead_letters() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;

    let id = queue::enqueue(&client, &Job::new("plane.jobs.echo").retry(2, 0, false))
        .await
        .expect("enqueue")
        .expect("inserted");

    let mut attempts = 0;
    loop {
        let jobs = queue::claim(&client, "worker", 1, 60).await.expect("claim");
        if jobs.is_empty() {
            break;
        }
        for job in jobs {
            attempts += 1;
            let failure = queue::fail(&client, "worker", &job, "boom", true)
                .await
                .expect("fail");
            if attempts < 3 {
                assert!(matches!(failure, Failure::Retry { .. }));
            } else {
                assert_eq!(failure, Failure::Dead);
            }
        }
    }
    assert_eq!(attempts, 3);
    let row = client
        .query_one(
            "SELECT state, attempts, last_error FROM plane_jobs.jobs WHERE id = $1",
            &[&id],
        )
        .await
        .expect("row");
    assert_eq!(row.get::<_, String>(0), "dead");
    assert_eq!(row.get::<_, i32>(1), 3);
    assert_eq!(row.get::<_, String>(2), "boom");
}

#[tokio::test]
async fn a_fatal_failure_is_dead_on_the_first_attempt() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;
    let id = queue::enqueue(&client, &Job::new("plane.jobs.echo").retry(5, 0, false))
        .await
        .expect("enqueue")
        .expect("inserted");
    let jobs = queue::claim(&client, "worker", 1, 60).await.expect("claim");
    let failure = queue::fail(&client, "worker", &jobs[0], "nope", false)
        .await
        .expect("fail");
    assert_eq!(failure, Failure::Dead);
    assert_eq!(
        client
            .query_one("SELECT state FROM plane_jobs.jobs WHERE id = $1", &[&id])
            .await
            .expect("row")
            .get::<_, String>(0),
        "dead"
    );
}

#[tokio::test]
async fn shutdown_releases_a_slow_in_flight_handler() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;
    let url = database_url().expect("url");
    let id = queue::enqueue(
        &client,
        &Job::new("plane.jobs.echo").payload(serde_json::json!({ "sleep_ms": 2_000 })),
    )
    .await
    .expect("enqueue")
    .expect("inserted");

    let settings = settings(&url);
    let pool = DbPools::from_settings(&settings).expect("pools");
    let worker = Worker::new(
        pool,
        registry(2_000),
        WorkerConfig {
            concurrency: 1,
            lease: Duration::from_secs(30),
            poll: Duration::from_millis(10),
            drain: Duration::from_millis(50),
        },
        "worker-shutdown".to_string(),
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(worker.run(
        async move {
            let _ = rx.await;
        },
        None,
    ));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let state: String = client
            .query_one("SELECT state FROM plane_jobs.jobs WHERE id = $1", &[&id])
            .await
            .expect("state")
            .get(0);
        if state == "running" {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "job never started");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tx.send(()).expect("shutdown");
    handle.await.expect("worker join").expect("worker run");

    let row = client
        .query_one(
            "SELECT state, attempts FROM plane_jobs.jobs WHERE id = $1",
            &[&id],
        )
        .await
        .expect("row");
    assert_eq!(row.get::<_, String>(0), "queued");
    assert_eq!(row.get::<_, i32>(1), 0);
}

#[tokio::test]
async fn two_beat_replicas_fire_each_tick_once_and_survive_a_kill() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;
    let url = database_url().expect("url");
    let settings = settings(&url);

    let beat_a = Beat::new(
        DbPools::from_settings(&settings).expect("pools a"),
        BeatConfig {
            poll: Duration::from_millis(50),
            handover_gap: Duration::from_secs(30),
            handover_margin: Duration::from_secs(15),
        },
        Some(1),
    )
    .await
    .expect("beat a");
    let beat_b = Beat::new(
        DbPools::from_settings(&settings).expect("pools b"),
        BeatConfig {
            poll: Duration::from_millis(50),
            handover_gap: Duration::from_secs(30),
            handover_margin: Duration::from_secs(15),
        },
        Some(1),
    )
    .await
    .expect("beat b");

    let (stop_a, wait_a) = tokio::sync::oneshot::channel::<()>();
    let (stop_b, wait_b) = tokio::sync::oneshot::channel::<()>();
    let handle_a = tokio::spawn(beat_a.run(async move {
        let _ = wait_a.await;
    }));
    let handle_b = tokio::spawn(beat_b.run(async move {
        let _ = wait_b.await;
    }));

    tokio::time::sleep(Duration::from_secs(4)).await;
    let before = count(
        &client,
        "SELECT count(*) FROM plane_jobs.ticks WHERE schedule_name = 'plane.jobs.test-tick'",
    )
    .await;
    assert!(before >= 2, "expected ticks, got {before}");

    stop_a.send(()).expect("stop a");
    handle_a.await.expect("join a").expect("beat a");
    let after_stop = count(
        &client,
        "SELECT count(*) FROM plane_jobs.ticks WHERE schedule_name = 'plane.jobs.test-tick'",
    )
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after = count(
        &client,
        "SELECT count(*) FROM plane_jobs.ticks WHERE schedule_name = 'plane.jobs.test-tick'",
    )
    .await;
    assert!(
        after > after_stop,
        "the surviving replica did not continue: {after_stop} -> {after}"
    );

    stop_b.send(()).expect("stop b");
    handle_b.await.expect("join b").expect("beat b");

    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM (SELECT schedule_name, tick_at FROM plane_jobs.ticks \
             GROUP BY schedule_name, tick_at HAVING count(*) > 1) duplicates",
        )
        .await,
        0
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM plane_jobs.jobs").await,
        count(&client, "SELECT count(*) FROM plane_jobs.ticks").await,
        "every tick must enqueue exactly one job"
    );
}

#[tokio::test]
async fn dispatch_routes_to_celery_or_the_queue_by_ownership() {
    let Some(client) = connect().await else {
        return;
    };
    let _guard = SERIAL.lock().await;
    reset(&client).await;

    let message = tasks::recent_visited_task(RecentVisitedTask::ProjectKeyed {
        slug: "acme".to_string(),
        project_id: None,
        entity_name: "project".to_string(),
        entity_identifier: TaskArg::str("p1"),
        user_id: TaskArg::str("u1"),
    });

    // A real task is Celery-owned: without a publisher the dispatch must not
    // fall through to Postgres or silently drop the message.
    let error = route::dispatch::<tokio_postgres::Client>(
        "plane.bgtasks.recent_visited_task.recent_visited_task",
        &Job::new("plane.bgtasks.recent_visited_task.recent_visited_task"),
        &message,
        None,
        Some(&client),
    )
    .await
    .expect_err("Celery-owned task without a publisher");
    assert!(matches!(error, route::DispatchError::NoPublisher(_)));

    // The test-only task is Rust-owned: the same API inserts a queue row, and
    // the worker drains it to `done`.
    let delivery = route::dispatch(
        "plane.jobs.echo",
        &Job::new("plane.jobs.echo"),
        &message,
        None,
        Some(&client),
    )
    .await
    .expect("rust-owned dispatch");
    let Delivery::Queued(Some(id)) = delivery else {
        panic!("expected a queued job, got {delivery:?}");
    };
    assert_eq!(
        count(&client, "SELECT count(*) FROM plane_jobs.jobs").await,
        1
    );

    let settings = settings(&database_url().expect("url"));
    let pool = DbPools::from_settings(&settings).expect("pools");
    let worker = Worker::new(
        pool,
        registry(0),
        WorkerConfig {
            concurrency: 1,
            lease: Duration::from_secs(30),
            poll: Duration::from_millis(10),
            drain: Duration::from_secs(1),
        },
        "worker-dispatch".to_string(),
    );
    assert_eq!(worker.drain().await.expect("drain"), 1);
    assert_eq!(
        client
            .query_one("SELECT state FROM plane_jobs.jobs WHERE id = $1", &[&id])
            .await
            .expect("row")
            .get::<_, String>(0),
        "done"
    );
}
