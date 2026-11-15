// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Latency of one `check_throttles` call against a real Redis.
//!
//! ```text
//! cd apps/server
//! CARGO_TARGET_DIR=target/throttle cargo run -p plane-throttle --release \
//!     --example latency -- redis://127.0.0.1:56379/10
//! ```
//!
//! Uses 10 000 distinct client addresses so every call is a *write* (GET, then
//! SET PX 60000) — the common case for anonymous traffic. A second pass reuses
//! one address, which is the read-only path every request after the limit takes.
//! Numbers are reported in microseconds; the harness prints p50, p99, p99.9 and
//! the mean.
//!
//! The example calls `FLUSHDB`, so it only accepts databases 9-11
//! (`apps/server/AGENTS.md`).

use std::time::Instant;

use http::HeaderMap;
use plane_core::config::ThrottlingSettings;
use plane_throttle::{
    Cache, ParsedUrl, Redis, StaticUser, ThrottleClass, ThrottleOutcome, ThrottleRequest,
    check_throttles, now,
};

const CALLS: usize = 10_000;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("PLANE_RS_TEST_REDIS_URL").ok())
        .expect("usage: latency <redis-url with db 9-11>");
    let database = ParsedUrl::parse(&url)?.db;
    assert!(
        matches!(database, 9..=11),
        "the latency example FLUSHDBs its database; use one of 9-11 (got {database})"
    );

    let redis = Redis::from_url(&url)?;
    redis.flush_db().await?;

    let settings = ThrottlingSettings {
        api_key_rate_limit: "60/minute".to_string(),
        authentication_rate_limit: "10/minute".to_string(),
        num_proxies: None,
        anon_rate: "30/minute",
        asset_rate: "5/minute",
        email_verification_rate: "3/hour",
    };
    let user = StaticUser::anonymous();
    let headers = HeaderMap::new();
    let cache = Cache::new(&redis);

    // Establish the connection before the clock starts.
    cache.get_history(Some("warmup")).await?;

    // ---- allowed path: one bucket per call ---------------------------------
    let mut samples = Vec::with_capacity(CALLS);
    let mut allowed = 0_usize;
    for index in 0..CALLS {
        let ident = format!(
            "10.{}.{}.{}",
            index / 65_536,
            (index / 256) % 256,
            index % 256
        );
        let request = ThrottleRequest {
            settings: &settings,
            user: &user,
            headers: &headers,
            remote_addr: Some(&ident),
            asset_id: None,
            scope_override: None,
        };
        let start = Instant::now();
        let outcome = check_throttles(&redis, &[ThrottleClass::Anon], &request, now()).await?;
        samples.push(start.elapsed().as_nanos());
        if matches!(outcome, ThrottleOutcome::Allowed { .. }) {
            allowed += 1;
        }
    }
    assert_eq!(allowed, CALLS, "every first request must be allowed");
    report(
        "check_throttles anon (GET + SET, all allowed)",
        &mut samples,
    );

    // ---- denied path: one bucket, read only ------------------------------
    let ident = "198.51.100.7".to_string();
    for _ in 0..31 {
        let request = ThrottleRequest {
            settings: &settings,
            user: &user,
            headers: &headers,
            remote_addr: Some(&ident),
            asset_id: None,
            scope_override: None,
        };
        let _ = check_throttles(&redis, &[ThrottleClass::Anon], &request, now()).await?;
    }
    let mut denied_samples = Vec::with_capacity(CALLS / 4);
    for _ in 0..(CALLS / 4) {
        let request = ThrottleRequest {
            settings: &settings,
            user: &user,
            headers: &headers,
            remote_addr: Some(&ident),
            asset_id: None,
            scope_override: None,
        };
        let start = Instant::now();
        let outcome = check_throttles(&redis, &[ThrottleClass::Anon], &request, now()).await?;
        denied_samples.push(start.elapsed().as_nanos());
        assert!(matches!(outcome, ThrottleOutcome::Throttled(_)));
    }
    report(
        "check_throttles anon (GET only, throttled)",
        &mut denied_samples,
    );

    redis.flush_db().await?;
    Ok(())
}

fn report(label: &str, samples: &mut [u128]) {
    samples.sort_unstable();
    let percentile = |p: f64| -> f64 {
        let index = ((samples.len() as f64 - 1.0) * p).round() as usize;
        samples[index] as f64 / 1_000.0
    };
    let mean = samples.iter().sum::<u128>() as f64 / samples.len() as f64 / 1_000.0;
    println!(
        "{label}: n={} p50={:.1}µs p99={:.1}µs p99.9={:.1}µs mean={:.1}µs",
        samples.len(),
        percentile(0.50),
        percentile(0.99),
        percentile(0.999),
        mean,
    );
}
