// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Measures the two costs the crate report asks about, on this machine:
//!
//! * the p50/p95 of SSRF validation (URL split, resolution + address check,
//!   webhook policy) and of building a TLS-configured `reqwest::Client`;
//! * the resident memory of a process that has done one request, so the
//!   reqwest+rustls footprint can be compared with `examples/minimal.rs`.
//!
//! Run with `cargo run --release -p plane-net --example cost`. The numbers in
//! `apps/server/BENCH.md`'s companion table (crate report) come from this
//! binary; re-run it rather than trusting them.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plane_net::HttpEngine;
use plane_net::client::OutboundRequest;
use plane_net::dns::{SharedResolver, StubResolver};
use plane_net::preset;
use plane_net::ssrf::SsrfPolicy;

const URLS: &[&str] = &[
    "https://hooks.example.com/services/T000/B000/XYZ",
    "https://api.example.com/v1/events?since=2026-01-01T00:00:00Z",
    "http://user:p%40ss@example.com:8080/a/b/c?x=1#frag",
    "https://93.184.216.34/callback",
    "https://münchen.example/hook",
    "http://[2606:4700:4700::1111]/x",
];

#[derive(Default)]
struct Latencies(Vec<Duration>);

impl Latencies {
    fn push(&mut self, duration: Duration) {
        self.0.push(duration);
    }

    fn report(&mut self, label: &str) {
        self.0.sort_unstable();
        let p50 = self.0[self.0.len() / 2];
        let p95 = self.0[self.0.len() * 95 / 100];
        let p99 = self.0[self.0.len() * 99 / 100];
        println!(
            "{label:<44} p50 {:>8.2} µs   p95 {:>8.2} µs   p99 {:>8.2} µs   (n={})",
            p50.as_secs_f64() * 1e6,
            p95.as_secs_f64() * 1e6,
            p99.as_secs_f64() * 1e6,
            self.0.len()
        );
    }
}

fn resolver() -> StubResolver {
    let hosts: BTreeMap<String, Vec<IpAddr>> =
        ["example.com", "hooks.example.com", "api.example.com"]
            .into_iter()
            .map(|host| (host.to_string(), vec!["93.184.216.34".parse().expect("ip")]))
            .collect();
    StubResolver::new(hosts, Some(vec!["93.184.216.34".parse().expect("ip")]))
}

fn resident_kib() -> Option<u64> {
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        return Some(pages * 4);
    }
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse().ok()
}

fn main() {
    let iterations: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(20_000);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // 1. URL splitting (`urllib.parse` port).
    let mut split = Latencies::default();
    for index in 0..iterations {
        let url = URLS[index % URLS.len()];
        let start = Instant::now();
        let _ = plane_net::split_target(url);
        split.push(start.elapsed());
    }
    split.report("split_target");

    // 2. Resolution + every-address validation, one resolver call per hop.
    let stub = resolver();
    let policy = SsrfPolicy::strict();
    let mut validate = Latencies::default();
    for index in 0..iterations {
        let url = URLS[index % URLS.len()];
        let start = Instant::now();
        runtime.block_on(async {
            let _ = plane_net::validate_hop(&stub, url, &policy).await;
        });
        validate.push(start.elapsed());
    }
    validate.report("validate_hop (resolve + address checks)");

    // 3. The complete create-time webhook policy, including the disallowed
    //    domain guard.
    let allowed_hosts = vec!["silo".to_string()];
    let policy = SsrfPolicy::new(vec!["10.0.0.0/8".parse().expect("net")], allowed_hosts);
    let mut webhook = Latencies::default();
    for index in 0..iterations {
        let url = URLS[index % URLS.len()];
        let start = Instant::now();
        runtime.block_on(async {
            let _ = plane_net::webhook::validate_webhook_url(
                &stub,
                url,
                &policy,
                &["example.com".to_string()],
                Some("plane.example.com"),
            )
            .await;
        });
        webhook.push(start.elapsed());
    }
    webhook.report("validate_webhook_url (create/update)");

    // 4. Building a TLS-configured client, the per-hop allocation the pinned
    //    transport pays (Python builds a `requests.Session` per hop as well).
    let tls = plane_net::TlsSettings::new();
    let config = tls.config(&plane_net::TlsTrust::Certifi).expect("tls");
    let mut build = Latencies::default();
    for _ in 0..2_000 {
        let start = Instant::now();
        let client = reqwest::ClientBuilder::new()
            .use_preconfigured_tls((*config).clone())
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("client");
        build.push(start.elapsed());
        drop(client);
    }
    build.report("build pinned reqwest client");

    // 5. Memory after one real request against a local plain-HTTP server would
    //    need a server; instead report the footprint once the TLS stack, root
    //    store and one client exist, which is what a cold worker pays.
    let engine = HttpEngine::new(Arc::new(resolver()) as SharedResolver);
    let _ = engine;
    let _ = preset::WEBHOOK;
    let _ = OutboundRequest::new();
    println!();
    match resident_kib() {
        Some(kib) => println!("resident after TLS setup: {:.1} MiB", kib as f64 / 1024.0),
        None => println!("resident: unavailable on this platform"),
    }
}
