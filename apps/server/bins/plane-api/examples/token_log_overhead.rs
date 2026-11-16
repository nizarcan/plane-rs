// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Measure `APITokenLogMiddleware`'s per-request cost on `/api/v1/`.
//!
//! Three numbers, because only the last one is I/O:
//!
//! * **build** — `ApiTokenLog::build`: HMAC of the API key, the Python
//!   `dict.__repr__` header column and `_safe_decode_body` over the response;
//! * **encode** — `Message::encode`: kombu's JSON body plus Celery's
//!   `argsrepr`/`kwargsrepr` (the latter truncates the whole `log_data` dict at
//!   1024 characters);
//! * **publish** — the awaited AMQP publish, which is what a request actually
//!   pays. It includes Celery's publish retry budget only when the broker is
//!   down; with a reachable broker it is one round trip and no confirm (Celery
//!   does not enable publisher confirms).
//!
//! ```sh
//! cd apps/server
//! eval "$(dev/django-env.sh)"           # DATABASE_URL, REDIS_URL, AMQP_URL…
//! cargo run -p plane-api --example token_log_overhead -- --publish 200
//! ```
//!
//! A typical v1 request has a small JSON body; the response here is the
//! compact user object `/api/v1/users/me/` returns, 166 bytes.

use std::time::Instant;

use anyhow::Context;
use plane_core::config::{Role, Settings};
use plane_drf::token_log::ApiTokenLog;
use plane_jobs::celery::message::EncodeContext;
use plane_jobs::celery::publisher::CeleryPublisher;

const RESPONSE_BODY: &[u8] = br#"{"id":"a5699706-ac3c-5956-9a38-09eaf4f9e07f","first_name":"Policy Api Active","last_name":"Probe","email":"policy_api_active@policy.test","avatar":"","avatar_url":null,"display_name":"Policy Api Active"}"#;
const REQUEST_BODY: &[u8] = br#"{"name":"probe"}"#;

fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let iterations = argument(&arguments, "--iterations").unwrap_or(20_000);
    let publishes = argument(&arguments, "--publish").unwrap_or(0);

    let settings = Settings::from_env(Role::Api)
        .context("settings (source apps/server/dev/django-env.sh first)")?;
    let request = request_context();
    let log = ApiTokenLog::build(&settings, &request, 200, RESPONSE_BODY)
        .context("the request must carry X-Api-Key")?;

    // ---- build -----------------------------------------------------------
    let start = Instant::now();
    for _ in 0..iterations {
        let built = ApiTokenLog::build(&settings, &request, 200, RESPONSE_BODY);
        std::hint::black_box(built);
    }
    report("build", start.elapsed(), iterations, log.headers.len());

    // ---- encode ----------------------------------------------------------
    let message = log.message();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;
    let context = EncodeContext {
        id: uuid::Uuid::nil(),
        now: chrono::Utc::now(),
        origin: "gen1@bench",
        reply_to: "bench",
        root_id: None,
    };
    let start = Instant::now();
    for _ in 0..iterations {
        let encoded = message.encode(&context).context("encode")?;
        std::hint::black_box(encoded.body.len());
    }
    report("encode", start.elapsed(), iterations, 0);
    let encoded = message.encode(&context).context("encode")?;
    println!("body bytes: {}", encoded.body.len());
    println!("kwargsrepr bytes: {}", encoded.headers.kwargsrepr.len());

    // ---- publish ---------------------------------------------------------
    if publishes > 0 {
        let publisher = CeleryPublisher::from_settings(&settings)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        runtime.block_on(async {
            publisher
                .connect()
                .await
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            let mut timings = Vec::with_capacity(publishes);
            for _ in 0..publishes {
                let start = Instant::now();
                publisher
                    .publish(&message)
                    .await
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                timings.push(start.elapsed());
            }
            report_durations("publish (awaited)", &mut timings);
            Ok::<(), anyhow::Error>(())
        })?;
        println!("broker: {}", publisher.broker_host());
    }
    Ok(())
}

fn argument(arguments: &[String], name: &str) -> Option<usize> {
    let position = arguments.iter().position(|value| value == name)?;
    arguments.get(position + 1)?.parse().ok()
}

/// A `POST /api/v1/workspaces/policy/projects/` carrying the fixture's active
/// API key: the shape the layer sees on a real v1 request.
fn request_context() -> plane_core::context::RequestContext {
    use http::{HeaderMap, HeaderValue, Method};
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("host", "plane-fixture.test"),
        ("accept", "application/json"),
        ("accept-encoding", "gzip"),
        ("user-agent", "v1-token-log-probe"),
        (
            "x-api-key",
            "f3fe6193a32f5f7ebd99c5d4d19751153957edbdb867560fa8f18d558995ad25",
        ),
        ("x-forwarded-for", "198.51.100.61"),
    ] {
        headers.insert(
            http::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
            HeaderValue::from_str(value).expect("header value"),
        );
    }
    let mut ctx = plane_core::context::RequestContext::new(
        Method::POST,
        "/api/v1/workspaces/policy/projects/",
        "",
        headers,
        None,
    );
    ctx.body = Some(REQUEST_BODY.to_vec());
    ctx
}

fn report(label: &str, elapsed: std::time::Duration, iterations: usize, extra: usize) {
    let per = elapsed.as_secs_f64() / iterations as f64;
    println!(
        "{label:<20} {iterations} iterations, {:.3} µs/request (total {:.1} ms){}",
        per * 1e6,
        elapsed.as_secs_f64() * 1e3,
        if extra > 0 {
            format!(", headers column {} bytes", extra)
        } else {
            String::new()
        }
    );
}

fn report_durations(label: &str, timings: &mut [std::time::Duration]) {
    timings.sort_unstable();
    let mean: f64 = timings
        .iter()
        .map(std::time::Duration::as_secs_f64)
        .sum::<f64>()
        / timings.len() as f64;
    let p50 = timings[timings.len() / 2].as_secs_f64();
    let p99 = timings[(timings.len() * 99 / 100).min(timings.len() - 1)].as_secs_f64();
    println!(
        "{label:<20} {} publishes: mean {:.3} ms, p50 {:.3} ms, p99 {:.3} ms",
        timings.len(),
        mean * 1e3,
        p50 * 1e3,
        p99 * 1e3
    );
}
