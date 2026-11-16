// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Cost of one presign with cached credentials (DECISIONS.md D6).
//!
//! ```sh
//! CARGO_TARGET_DIR=target/storage cargo run --release -p plane-storage --example bench_presign
//! ```
//!
//! Both variants are measured twice: with a frozen `now` (the work the crate
//! does) and with `Utc::now()` per call (what a request path pays). No network
//! is touched: credentials come from the environment and are cached after the
//! first call.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use plane_core::config::{EnvMap, Role, Settings};
use plane_storage::{PresignGet, PresignPost, Storage};

const ITERATIONS: usize = 50_000;

fn percentile(sorted: &[Duration], percent: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * percent).round() as usize;
    sorted[index]
}

fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort_unstable();
    let total: Duration = samples.iter().sum();
    println!(
        "{label}: p50 {:.2} µs, p95 {:.2} µs, p99 {:.2} µs, mean {:.2} µs",
        percentile(&samples, 0.50).as_secs_f64() * 1e6,
        percentile(&samples, 0.95).as_secs_f64() * 1e6,
        percentile(&samples, 0.99).as_secs_f64() * 1e6,
        total.as_secs_f64() * 1e6 / samples.len() as f64,
    );
}

#[tokio::main]
async fn main() {
    let env = EnvMap::new()
        .set("SECRET_KEY", "bench")
        .set("REDIS_URL", "redis://127.0.0.1:56379/15")
        .set("AWS_ACCESS_KEY_ID", "access-key")
        .set("AWS_SECRET_ACCESS_KEY", "secret-key")
        .set("AWS_S3_BUCKET_NAME", "uploads")
        .set("AWS_REGION", "us-east-1")
        .set("AWS_S3_ENDPOINT_URL", "http://127.0.0.1:59000");
    let settings = Settings::from_source(&env, Role::Worker).unwrap();
    let storage = Storage::new(&settings.storage, &env).unwrap();
    let view = storage.worker();
    let frozen: DateTime<Utc> = "2026-11-16T12:34:56Z".parse().unwrap();

    for _ in 0..1_000 {
        let _ = view
            .presign_get_at(
                frozen,
                PresignGet {
                    key: "ws/0f8f-file name.png",
                    expiration: None,
                    disposition: "attachment",
                    filename: Some("file name.png"),
                    content_type: Some("image/png"),
                },
            )
            .await
            .unwrap();
        let _ = view
            .presign_post_at(
                frozen,
                PresignPost {
                    key: "ws/0f8f-file name.png",
                    file_type: Some("image/png"),
                    file_size: 5242880,
                    expiration: None,
                },
            )
            .await
            .unwrap();
    }

    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let _ = view
            .presign_get_at(
                frozen,
                PresignGet {
                    key: "ws/0f8f-file name.png",
                    expiration: None,
                    disposition: "attachment",
                    filename: Some("file name.png"),
                    content_type: Some("image/png"),
                },
            )
            .await
            .unwrap();
        samples.push(start.elapsed());
    }
    report("presign_get (frozen clock)", samples);

    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let _ = view
            .presign_post_at(
                frozen,
                PresignPost {
                    key: "ws/0f8f-file name.png",
                    file_type: Some("image/png"),
                    file_size: 5242880,
                    expiration: None,
                },
            )
            .await
            .unwrap();
        samples.push(start.elapsed());
    }
    report("presign_post (frozen clock)", samples);

    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let now = Utc::now();
        let start = Instant::now();
        let _ = view
            .presign_get_at(
                now,
                PresignGet {
                    key: "ws/0f8f-file name.png",
                    expiration: None,
                    disposition: "inline",
                    filename: Some("file name.png"),
                    content_type: Some("image/png"),
                },
            )
            .await
            .unwrap();
        samples.push(start.elapsed());
    }
    report("presign_get (Utc::now per call, inline)", samples);
}
