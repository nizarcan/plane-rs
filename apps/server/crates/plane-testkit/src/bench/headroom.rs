// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The client's own throughput, measured against a server that does nothing.
//!
//! The point is to bound the instrument: if the load generator tops out at
//! 30k req/s at concurrency 64, a target that reports 29k is being measured
//! near the client's ceiling and its real capacity is unknown. `BENCH.md`
//! records this table next to the real numbers so no row is over-read.
//!
//! The trivial server shares the machine with the client, so this is a
//! conservative bound: both the client and the server pay for the same cores.

use std::time::Duration;

use anyhow::Result;

use crate::bench::load;
use crate::bench::report::HeadroomPoint;
use crate::bench::request::RequestBlueprint;
use crate::bench::server::TrivialServer;

/// Run the bench loop against a local `200 OK` server at each concurrency.
pub async fn measure(concurrencies: &[usize], duration: Duration) -> Result<Vec<HeadroomPoint>> {
    let server = TrivialServer::start(200, false).await?;
    let blueprints = vec![RequestBlueprint::simple(&server.base_url, "/headroom")?];
    let authority = server
        .base_url
        .split_once("://")
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| server.base_url.clone());
    let mut points = Vec::with_capacity(concurrencies.len());
    for concurrency in concurrencies {
        let run = load::run(
            &blueprints,
            &[0],
            server.addr,
            &authority,
            *concurrency,
            duration,
        )
        .await?;
        let summary = run.latencies.summary();
        points.push(HeadroomPoint {
            concurrency: *concurrency,
            requests: run.requests,
            errors: run.errors,
            rps: run.rps(),
            p50_ms: summary.p50_ms,
            p95_ms: summary.p95_ms,
            p99_ms: summary.p99_ms,
        });
    }
    Ok(points)
}
