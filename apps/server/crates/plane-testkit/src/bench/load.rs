// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The closed-loop load generator.
//!
//! One connection per concurrent worker, HTTP/1.1 keep-alive, one request in
//! flight per connection: that is the definition of closed loop, and it means
//! the reported requests/s is "what a client that waits for each response can
//! do", not an open-loop injection rate the target may never have a chance to
//! catch up with.
//!
//! The hot loop allocates as little as possible:
//!
//! * every request template is pre-rendered (`request::RequestBlueprint`), so
//!   no path, header or body is formatted per request;
//! * the request is a `Clone` of a prepared `Request<Full<Bytes>>` (a small
//!   header-map clone plus a refcounted `Bytes` body);
//! * the response body is drained frame by frame (`BodyExt::frame`) instead of
//!   `collect()`, so an empty body costs no allocation;
//! * one `Instant` pair per request, recorded in an HDR histogram that does
//!   not grow with the sample count.
//!
//! A connection the server closed mid-run is re-opened and the request is
//! retried once; a retry is counted as a reconnect, not as an error, because
//! gunicorn's and uvicorn's keep-alive timers close idle connections by
//! design.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::client::conn::http1::SendRequest;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

use crate::bench::histogram::Latencies;
use crate::bench::request::RequestBlueprint;

/// Everything one measured (or warm-up) window produced.
#[derive(Debug)]
pub struct LoadRun {
    /// Requests issued during the window, successful or not.
    pub requests: u64,
    /// Requests whose status differed from the template's `expect_status`, or
    /// that failed at the transport level after one reconnect and retry.
    pub errors: u64,
    /// Connections re-opened because the server closed the previous one.
    pub reconnects: u64,
    /// Requests per template, in blueprint order.
    pub per_template: Vec<u64>,
    /// Response latency (send to last body frame). Transport failures are not
    /// sampled here; they are counted in `errors`.
    pub latencies: Latencies,
    /// Wall time of the slowest worker's measured window.
    pub elapsed: Duration,
}

impl LoadRun {
    pub fn rps(&self) -> f64 {
        if self.elapsed.is_zero() {
            0.0
        } else {
            self.requests as f64 / self.elapsed.as_secs_f64()
        }
    }

    fn empty(templates: usize) -> Self {
        Self {
            requests: 0,
            errors: 0,
            reconnects: 0,
            per_template: vec![0; templates],
            latencies: Latencies::new(),
            elapsed: Duration::ZERO,
        }
    }

    fn merge(&mut self, other: LoadRun) {
        self.requests += other.requests;
        self.errors += other.errors;
        self.reconnects += other.reconnects;
        for (slot, count) in self.per_template.iter_mut().zip(other.per_template) {
            *slot += count;
        }
        self.latencies.merge(&other.latencies);
        self.elapsed = self.elapsed.max(other.elapsed);
    }
}

struct Conn {
    sender: SendRequest<Full<Bytes>>,
    /// The connection future must be polled for the sender to make progress.
    _task: JoinHandle<()>,
}

impl Conn {
    async fn connect(addr: SocketAddr, authority: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connecting to {addr}"))?;
        stream.set_nodelay(true).context("setting TCP_NODELAY")?;
        let (sender, connection) = hyper::client::conn::http1::Builder::new()
            .handshake::<_, Full<Bytes>>(TokioIo::new(stream))
            .await
            .with_context(|| format!("HTTP handshake with {authority}"))?;
        // The task ends when either side closes the connection.
        let task = tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Self {
            sender,
            _task: task,
        })
    }
}

/// Send one request and drain its body. Returns the status code.
async fn attempt(conn: &mut Conn, request: Request<Full<Bytes>>) -> Result<u16> {
    let response = conn.sender.send_request(request).await.context("send")?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    while let Some(frame) = body.frame().await {
        frame.context("reading the response body")?;
    }
    Ok(status)
}

/// xorshift64*, so the packet mix needs no dependency and no allocation. The
/// seed differs per worker: workers must not all pick the same template on the
/// same tick.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.0 = value;
        value.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

struct WorkerOutcome {
    run: LoadRun,
}

async fn worker(
    blueprints: Arc<Vec<RequestBlueprint>>,
    schedule: Arc<Vec<usize>>,
    addr: SocketAddr,
    authority: Arc<String>,
    seed: u64,
    duration: Duration,
) -> Result<WorkerOutcome> {
    let mut conn = Conn::connect(addr, &authority)
        .await
        .context("the bench client could not open its connection")?;
    let mut rng = Rng::new(seed);
    let mut run = LoadRun::empty(blueprints.len());

    // The measured window starts once this worker's connection is up, so
    // connection setup is not counted as request latency or stretched into
    // the reported rate.
    let started = Instant::now();
    let deadline = started + duration;
    while Instant::now() < deadline {
        let slot = (rng.next() as usize) % schedule.len();
        let index = schedule[slot];
        let template = &blueprints[index];
        let request_started = Instant::now();
        let mut result = attempt(&mut conn, template.request()).await;
        if result.is_err() {
            // A keep-alive connection the server closed is expected; reconnect
            // and retry the same request exactly once.
            run.reconnects += 1;
            match Conn::connect(addr, &authority).await {
                Ok(mut fresh) => {
                    result = attempt(&mut fresh, template.request()).await;
                    conn = fresh;
                }
                Err(error) => {
                    result = Err(error);
                }
            }
        }
        let latency = request_started.elapsed();

        run.requests += 1;
        run.per_template[index] += 1;
        match result {
            Ok(status) => {
                run.latencies.record(latency);
                if status != template.expect_status {
                    run.errors += 1;
                }
            }
            Err(_) => run.errors += 1,
        }
    }
    run.elapsed = started.elapsed();
    Ok(WorkerOutcome { run })
}

/// Run `concurrency` closed-loop workers for `duration` and merge their
/// results. Each worker owns one keep-alive connection.
pub async fn run(
    blueprints: &[RequestBlueprint],
    schedule: &[usize],
    addr: SocketAddr,
    authority: &str,
    concurrency: usize,
    duration: Duration,
) -> Result<LoadRun> {
    if concurrency == 0 {
        bail!("concurrency must be at least 1");
    }
    if blueprints.is_empty() || schedule.is_empty() {
        bail!("the load generator needs at least one request template");
    }
    let blueprints = Arc::new(blueprints.to_vec());
    let schedule = Arc::new(schedule.to_vec());
    let authority = Arc::new(authority.to_string());

    let mut handles = Vec::with_capacity(concurrency);
    for worker_index in 0..concurrency {
        let blueprints = Arc::clone(&blueprints);
        let schedule = Arc::clone(&schedule);
        let authority = Arc::clone(&authority);
        handles.push(tokio::spawn(async move {
            worker(
                blueprints,
                schedule,
                addr,
                authority,
                0x9E37_79B9_7F4A_7C15
                    ^ (worker_index as u64 + 1).wrapping_mul(0xBF58_476D_1CE4_E5B9),
                duration,
            )
            .await
        }));
    }

    let mut merged = LoadRun::empty(blueprints.len());
    let mut failures = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(outcome)) => merged.merge(outcome.run),
            Ok(Err(error)) => failures.push(format!("{error:#}")),
            Err(error) => failures.push(format!("worker task panicked: {error}")),
        }
    }
    if !failures.is_empty() {
        return Err(anyhow!(
            "{} of {concurrency} load worker(s) failed: {}",
            failures.len(),
            failures.join("; ")
        ));
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::request::RequestBlueprint;
    use crate::bench::server::TrivialServer;

    fn blueprint(base_url: &str, path: &str, expect_status: u16) -> RequestBlueprint {
        let mut blueprint = RequestBlueprint::simple(base_url, path).expect("blueprint");
        blueprint.expect_status = expect_status;
        blueprint
    }

    #[tokio::test]
    async fn a_keep_alive_server_is_driven_at_every_concurrency() {
        let server = TrivialServer::start(200, false).await.expect("server");
        let blueprints = vec![blueprint(&server.base_url, "/health", 200)];
        for concurrency in [1, 4] {
            let run = run(
                &blueprints,
                &[0],
                server.addr,
                &server.base_url[7..],
                concurrency,
                Duration::from_millis(150),
            )
            .await
            .expect("load run");
            assert!(run.requests > 0, "no requests at concurrency {concurrency}");
            assert_eq!(run.errors, 0);
            assert_eq!(run.reconnects, 0);
            assert_eq!(run.per_template, vec![run.requests]);
            assert_eq!(run.latencies.len(), run.requests);
            assert!(run.latencies.summary().p50_ms > 0.0);
            assert!(run.rps() > 0.0);
        }
    }

    #[tokio::test]
    async fn a_status_the_template_does_not_expect_is_an_error() {
        let server = TrivialServer::start(503, false).await.expect("server");
        let blueprints = vec![blueprint(&server.base_url, "/", 200)];
        let run = run(
            &blueprints,
            &[0],
            server.addr,
            &server.base_url[7..],
            2,
            Duration::from_millis(100),
        )
        .await
        .expect("load run");
        assert!(run.requests > 0);
        assert_eq!(run.errors, run.requests);
        // The status arrived, so latency was sampled even though it is an error.
        assert_eq!(run.latencies.len(), run.requests);
    }

    #[tokio::test]
    async fn a_closed_keep_alive_connection_is_reopened_without_an_error() {
        let server = TrivialServer::start(200, true).await.expect("server");
        let blueprints = vec![blueprint(&server.base_url, "/", 200)];
        let run = run(
            &blueprints,
            &[0],
            server.addr,
            &server.base_url[7..],
            1,
            Duration::from_millis(300),
        )
        .await
        .expect("load run");
        // Every response carries `Connection: close`, so every request after
        // the first one has to reconnect.
        assert!(run.reconnects >= 1, "reconnects: {}", run.reconnects);
        assert_eq!(run.errors, 0);
        assert!(run.requests > 1, "requests: {}", run.requests);
    }

    #[tokio::test]
    async fn a_target_that_is_down_fails_loudly_instead_of_reporting_zero() {
        // Bind a port, learn it, then close the listener: nothing is listening.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let blueprints = vec![blueprint("http://127.0.0.1:1", "/", 200)];
        let error = run(
            &blueprints,
            &[0],
            addr,
            "127.0.0.1",
            1,
            Duration::from_millis(50),
        )
        .await
        .expect_err("must fail");
        assert!(error.to_string().contains("failed"), "{error}");
    }

    #[test]
    fn the_worker_prng_does_not_stick_to_one_slot() {
        let mut rng = Rng::new(42);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            seen.insert(rng.next() % 4);
        }
        assert_eq!(seen.len(), 4);
    }
}
