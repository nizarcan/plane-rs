// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Publish 1,000 messages and report latency percentiles and RSS.
//!
//! ```sh
//! cargo run --release -p plane-jobs --example publish_bench -- \
//!     --url amqp://plane:plane@127.0.0.1:55672/t_celery_bench --count 1000
//! ```
//!
//! "Latency" is one `publish_encoded` call end to end: render is excluded (the
//! same `EncodedMessage` is reused) and the measured span includes lapin
//! writing the frames. Publisher confirms are off, exactly like Celery, so the
//! number is a send-side cost, not an ack round trip.

use std::error::Error;
use std::time::{Duration, Instant};

use plane_jobs::celery::publisher::CeleryPublisher;
use plane_jobs::celery::tasks::{IssueActivityBase, IssueActivityCall, issue_activity};

fn rss_kib() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .ok()
}

fn percentile(sorted: &[Duration], percent: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (percent / 100.0) * (sorted.len() - 1) as f64;
    let index = rank.round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut url = None;
    let mut count = 1_000usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => url = args.next(),
            "--count" => {
                count = args
                    .next()
                    .ok_or("--count needs a value")?
                    .parse::<usize>()?
            }
            other => return Err(format!("unknown argument {other:?}").into()),
        }
    }
    let url = url.ok_or("--url is required")?;

    let message = issue_activity(IssueActivityCall::view(
        IssueActivityBase {
            activity_type: "issue.activity.updated".to_string(),
            requested_data: plane_jobs::celery::TaskArg::json_text(r#"{"priority": "high"}"#),
            actor_id: "0d1e2f30-4456-4789-9abc-def012345678".to_string(),
            issue_id: plane_jobs::celery::TaskArg::str("11111111-2222-3333-4444-555555555555"),
            project_id: plane_jobs::celery::TaskArg::str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            current_instance: plane_jobs::celery::TaskArg::json_text(r#"{"priority": "medium"}"#),
            epoch: 1_794_764_767,
        },
        "http://localhost:3000",
    ));
    let encoded = message.encode(&plane_jobs::celery::EncodeContext {
        id: uuid::Uuid::new_v4(),
        now: chrono::Utc::now(),
        origin: "gen1234@bench",
        reply_to: "00000000-0000-4000-8000-000000000000",
        root_id: None,
    })?;

    let before = rss_kib();
    let publisher = CeleryPublisher::from_url(url)?;
    publisher.connect().await?;
    let connected = rss_kib();

    // One warm-up publish so lazily allocated buffers are not in the percentiles.
    publisher.publish_encoded(&encoded).await?;

    let mut samples = Vec::with_capacity(count);
    let started = Instant::now();
    for _ in 0..count {
        let start = Instant::now();
        publisher.publish_encoded(&encoded).await?;
        samples.push(start.elapsed());
    }
    let elapsed = started.elapsed();
    let after = rss_kib();

    samples.sort_unstable();
    let total_micros: u128 = samples.iter().map(Duration::as_micros).sum();
    println!("messages: {count}");
    println!("body_bytes: {}", encoded.body.len());
    println!(
        "p50: {:.3} ms",
        percentile(&samples, 50.0).as_secs_f64() * 1_000.0
    );
    println!(
        "p99: {:.3} ms",
        percentile(&samples, 99.0).as_secs_f64() * 1_000.0
    );
    println!(
        "mean: {:.3} ms",
        total_micros as f64 / samples.len().max(1) as f64 / 1_000.0
    );
    println!(
        "throughput: {:.0} msg/s",
        count as f64 / elapsed.as_secs_f64()
    );
    println!("rss: before={before:?} KiB connected={connected:?} KiB after={after:?} KiB");
    Ok(())
}
