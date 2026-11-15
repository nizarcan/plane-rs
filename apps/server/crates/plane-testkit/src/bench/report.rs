// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The JSON report and the Markdown table `difftest bench` produces.
//!
//! Every measurement is kept (one per repetition) and a median row is derived
//! per concurrency. `BENCH.md` is filled from the median rows; the per-run
//! values stay in the JSON so a suspicious median can be traced to the run
//! that produced it.
//!
//! Aggregation rule, chosen to be honest rather than flattering:
//!
//! * throughput, latency, memory and CPU are the **median** of the
//!   repetitions;
//! * `errors` and `reconnects` are the **worst** repetition, so a run that
//!   only failed once still shows up in the summary instead of disappearing
//!   behind two clean runs.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::bench::histogram::{median, median_u64};

pub const REPORT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub os: String,
    pub arch: String,
    pub cpus: usize,
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetInfo {
    /// Target name from `targets.yaml` (`django`, `rust`) or `attach`.
    pub name: String,
    pub kind: String,
    pub base_url: String,
    /// Database the target served. In attach mode this is the name the caller
    /// declared; the harness cannot verify it.
    pub database: String,
    pub workers: Option<u32>,
    pub threads: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemeInfo {
    pub scenario: String,
    pub template_db: String,
    pub setup_steps: usize,
    pub request_templates: Vec<String>,
    pub duration_secs: f64,
    pub warmup_secs: f64,
    pub settle_secs: f64,
    pub repetitions: u32,
    pub concurrencies: Vec<usize>,
    pub redis_db: i64,
    /// How the client behaves; copied into `BENCH.md` so a row cannot be read
    /// without its definition.
    pub client: String,
    /// How often the process tree was sampled.
    pub rss_sample_interval_ms: u64,
}

/// One (concurrency, repetition) result, or the median row for a concurrency.
#[derive(Debug, Clone, Serialize)]
pub struct Measurement {
    pub scenario: String,
    pub target: String,
    pub target_kind: String,
    pub workers: Option<u32>,
    pub threads: Option<u32>,
    pub concurrency: usize,
    /// `Some(n)` for one repetition, `None` for the median row.
    pub repetition: Option<u32>,
    pub requests: u64,
    pub errors: u64,
    pub reconnects: u64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub idle_rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub after_rss_bytes: u64,
    pub cpu_secs: f64,
    pub processes_peak: u64,
    pub duration_secs: f64,
    pub per_template: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeadroomPoint {
    pub concurrency: usize,
    pub requests: u64,
    pub errors: u64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub schema: u32,
    pub tool: String,
    pub generated_at: String,
    pub host: HostInfo,
    pub target: TargetInfo,
    pub scheme: SchemeInfo,
    /// 1-minute load average before the first measurement: the shared machine
    /// may be running other work, and a reader deserves to know.
    pub load_average_start: Option<f64>,
    pub load_average_end: Option<f64>,
    pub measurements: Vec<Measurement>,
    pub medians: Vec<Measurement>,
    /// The client's own throughput against a trivial local server, measured
    /// with the same loop and connections.
    pub headroom: Vec<HeadroomPoint>,
    /// Caveats that apply to this particular run (a target that exited, an
    /// attach-mode target with no PID, ...).
    pub notes: Vec<String>,
}

impl BenchReport {
    pub fn write_json(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self).context("serialising the bench report")?;
        create_parent(path)?;
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
    }

    pub fn write_markdown(&self, path: &Path) -> Result<()> {
        let text = self.render_markdown();
        create_parent(path)?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// The Markdown table for this run, with the client-headroom table and the
    /// caveats below it.
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "## {} — {} ({})\n\n",
            self.scheme.scenario, self.target.name, self.target.kind
        ));
        out.push_str(&format!(
            "{} · {} vCPU · {} · {} repetitions · {}s measured after {}s warm-up · {}s settle · load avg {} → {} · client: {}\n\n",
            self.host.os,
            self.host.cpus,
            self.target_label(),
            self.scheme.repetitions,
            self.scheme.duration_secs,
            self.scheme.warmup_secs,
            self.scheme.settle_secs,
            optional_number(self.load_average_start),
            optional_number(self.load_average_end),
            self.scheme.client,
        ));
        out.push_str(
            "| scenario | target | workers/threads | conc | rps | p50 ms | p90 ms | p95 ms | p99 ms | max ms | errors | idle RSS | peak RSS | after RSS | CPU s | rps (reps) |\n",
        );
        out.push_str(
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
        );
        for median in &self.medians {
            let repetitions: Vec<String> = self
                .measurements
                .iter()
                .filter(|measurement| {
                    measurement.repetition.is_some()
                        && measurement.concurrency == median.concurrency
                })
                .map(|measurement| format!("{:.0}", measurement.rps))
                .collect();
            out.push_str(&format!(
                "| {} | {} | {} | {} | {:.0} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} | {} | {} | {:.2} | {} |\n",
                median.scenario,
                median.target,
                worker_label(median.workers, median.threads),
                median.concurrency,
                median.rps,
                median.p50_ms,
                median.p90_ms,
                median.p95_ms,
                median.p99_ms,
                median.max_ms,
                median.errors,
                mib(median.idle_rss_bytes),
                mib(median.peak_rss_bytes),
                mib(median.after_rss_bytes),
                median.cpu_secs,
                repetitions.join("/"),
            ));
        }
        if !self.headroom.is_empty() {
            out.push_str(
                "\nClient headroom (the same loop against a trivial local keep-alive server):\n\n",
            );
            out.push_str("| conc | rps | p50 ms | p95 ms | p99 ms | errors |\n");
            out.push_str("| --- | --- | --- | --- | --- | --- |\n");
            for point in &self.headroom {
                out.push_str(&format!(
                    "| {} | {:.0} | {:.2} | {:.2} | {:.2} | {} |\n",
                    point.concurrency,
                    point.rps,
                    point.p50_ms,
                    point.p95_ms,
                    point.p99_ms,
                    point.errors
                ));
            }
        }
        if !self.notes.is_empty() {
            out.push_str("\nNotes:\n\n");
            for note in &self.notes {
                out.push_str(&format!("- {note}\n"));
            }
        }
        out
    }

    fn target_label(&self) -> String {
        match (self.target.workers, self.target.threads) {
            (Some(workers), _) => format!("{} worker(s)", workers),
            (_, Some(threads)) => format!("{threads} thread(s)"),
            _ => "default threads".to_string(),
        }
    }
}

fn worker_label(workers: Option<u32>, threads: Option<u32>) -> String {
    match (workers, threads) {
        (Some(workers), _) => format!("workers={workers}"),
        (_, Some(threads)) => format!("threads={threads}"),
        _ => "default".to_string(),
    }
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn optional_number(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

/// Derive one median row per concurrency, in the order given.
pub fn medians(measurements: &[Measurement], concurrencies: &[usize]) -> Vec<Measurement> {
    let mut rows = Vec::with_capacity(concurrencies.len());
    for concurrency in concurrencies {
        let group: Vec<&Measurement> = measurements
            .iter()
            .filter(|measurement| measurement.concurrency == *concurrency)
            .collect();
        let Some(first) = group.first() else {
            continue;
        };
        let rps: Vec<f64> = group.iter().map(|measurement| measurement.rps).collect();
        let percentile = |value: fn(&Measurement) -> f64| {
            median(
                &group
                    .iter()
                    .map(|measurement| value(measurement))
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default()
        };
        let cpu: Vec<f64> = group
            .iter()
            .map(|measurement| measurement.cpu_secs)
            .collect();
        let mut per_template: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        for measurement in &group {
            for (name, count) in &measurement.per_template {
                per_template.entry(name.clone()).or_default().push(*count);
            }
        }
        rows.push(Measurement {
            scenario: first.scenario.clone(),
            target: first.target.clone(),
            target_kind: first.target_kind.clone(),
            workers: first.workers,
            threads: first.threads,
            concurrency: *concurrency,
            repetition: None,
            requests: median_u64(
                &group
                    .iter()
                    .map(|measurement| measurement.requests)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            // Worst case, not the median: one failed run must not vanish.
            errors: group
                .iter()
                .map(|measurement| measurement.errors)
                .max()
                .unwrap_or_default(),
            reconnects: group
                .iter()
                .map(|measurement| measurement.reconnects)
                .max()
                .unwrap_or_default(),
            rps: median(&rps).unwrap_or_default(),
            p50_ms: percentile(|measurement| measurement.p50_ms),
            p90_ms: percentile(|measurement| measurement.p90_ms),
            p95_ms: percentile(|measurement| measurement.p95_ms),
            p99_ms: percentile(|measurement| measurement.p99_ms),
            max_ms: percentile(|measurement| measurement.max_ms),
            mean_ms: percentile(|measurement| measurement.mean_ms),
            idle_rss_bytes: median_u64(
                &group
                    .iter()
                    .map(|measurement| measurement.idle_rss_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            peak_rss_bytes: median_u64(
                &group
                    .iter()
                    .map(|measurement| measurement.peak_rss_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            after_rss_bytes: median_u64(
                &group
                    .iter()
                    .map(|measurement| measurement.after_rss_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            cpu_secs: median(&cpu).unwrap_or_default(),
            processes_peak: median_u64(
                &group
                    .iter()
                    .map(|measurement| measurement.processes_peak)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            duration_secs: percentile(|measurement| measurement.duration_secs),
            per_template: per_template
                .into_iter()
                .map(|(name, counts)| (name, median_u64(&counts).unwrap_or_default()))
                .collect(),
        });
    }
    rows
}

/// Best-effort host description for the report header. Failures are not
/// errors: the numbers matter more than the label.
pub fn host_info() -> HostInfo {
    let os = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| format!("macOS {}", String::from_utf8_lossy(&output.stdout).trim()))
        .unwrap_or_else(|| {
            Command::new("uname")
                .args(["-sr"])
                .output()
                .ok()
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });
    HostInfo {
        os,
        arch: std::env::consts::ARCH.to_string(),
        cpus: std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1),
        memory_bytes: total_memory_bytes(),
    }
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    let output = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

#[cfg(not(target_os = "macos"))]
fn total_memory_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kib * 1024);
        }
    }
    None
}

/// The 1-minute load average, as a rough record of how busy the shared
/// machine was. A benchmark on a laptop is never quiet; this makes the noise
/// visible instead of pretending it is not there.
pub fn load_average() -> Option<f64> {
    #[cfg(target_os = "macos")]
    let output = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()?;
    #[cfg(not(target_os = "macos"))]
    let output = Command::new("cat").arg("/proc/loadavg").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace().find_map(|word| {
        word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
            .parse()
            .ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement(concurrency: usize, repetition: u32, rps: f64, errors: u64) -> Measurement {
        Measurement {
            scenario: "health".to_string(),
            target: "rust".to_string(),
            target_kind: "rust".to_string(),
            workers: None,
            threads: Some(4),
            concurrency,
            repetition: Some(repetition),
            requests: 1000,
            errors,
            reconnects: 0,
            rps,
            p50_ms: 0.2,
            p90_ms: 0.3,
            p95_ms: 0.4,
            p99_ms: 0.5,
            max_ms: 1.0,
            mean_ms: 0.25,
            idle_rss_bytes: 18 * 1024 * 1024,
            peak_rss_bytes: 24 * 1024 * 1024,
            after_rss_bytes: 20 * 1024 * 1024,
            cpu_secs: 2.0,
            processes_peak: 1,
            duration_secs: 20.0,
            per_template: BTreeMap::from([("health".to_string(), 1000)]),
        }
    }

    #[test]
    fn medians_are_per_concurrency_and_keep_the_worst_error_count() {
        let measurements = vec![
            measurement(1, 1, 100.0, 0),
            measurement(1, 2, 120.0, 3),
            measurement(1, 3, 110.0, 0),
            measurement(8, 1, 800.0, 0),
            measurement(8, 2, 900.0, 0),
            measurement(8, 3, 1000.0, 1),
        ];
        let rows = medians(&measurements, &[1, 8]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].concurrency, 1);
        assert_eq!(rows[0].rps, 110.0);
        assert_eq!(rows[0].errors, 3, "the median must keep the worst error");
        assert_eq!(rows[1].concurrency, 8);
        assert_eq!(rows[1].rps, 900.0);
        assert_eq!(rows[1].errors, 1);
        assert!(rows.iter().all(|row| row.repetition.is_none()));
    }

    #[test]
    fn a_single_repetition_is_its_own_median() {
        let rows = medians(&[measurement(64, 1, 1234.0, 0)], &[64]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rps, 1234.0);
        assert_eq!(rows[0].per_template.get("health"), Some(&1000));
    }

    fn report() -> BenchReport {
        let measurements = vec![
            measurement(1, 1, 100.0, 0),
            measurement(1, 2, 120.0, 0),
            measurement(1, 3, 110.0, 0),
        ];
        let medians = medians(&measurements, &[1]);
        BenchReport {
            schema: REPORT_SCHEMA,
            tool: "difftest 0.1.0".to_string(),
            generated_at: "2026-11-15T00:00:00Z".to_string(),
            host: HostInfo {
                os: "macOS 26.6.2".to_string(),
                arch: "aarch64".to_string(),
                cpus: 14,
                memory_bytes: Some(36 * 1024 * 1024 * 1024),
            },
            target: TargetInfo {
                name: "rust".to_string(),
                kind: "rust".to_string(),
                base_url: "http://127.0.0.1:18100".to_string(),
                database: "bench_1".to_string(),
                workers: None,
                threads: Some(4),
            },
            scheme: SchemeInfo {
                scenario: "health".to_string(),
                template_db: "plane_tpl".to_string(),
                setup_steps: 0,
                request_templates: vec!["health".to_string()],
                duration_secs: 20.0,
                warmup_secs: 5.0,
                settle_secs: 10.0,
                repetitions: 3,
                concurrencies: vec![1],
                redis_db: 3,
                client: "closed-loop".to_string(),
                rss_sample_interval_ms: 250,
            },
            measurements,
            medians,
            headroom: vec![HeadroomPoint {
                concurrency: 1,
                requests: 100_000,
                errors: 0,
                rps: 50_000.0,
                p50_ms: 0.02,
                p95_ms: 0.03,
                p99_ms: 0.05,
            }],
            load_average_start: Some(1.5),
            load_average_end: Some(2.5),
            notes: vec!["a note".to_string()],
        }
    }

    #[test]
    fn the_markdown_table_carries_every_column_the_brief_asks_for() {
        let text = report().render_markdown();
        for expected in [
            "health",
            "| 1 |",
            "110",
            "0.50",
            "18.0 MiB",
            "24.0 MiB",
            "2.00",
            "100/120/110",
            "Client headroom",
            "50000",
            "a note",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
    }

    #[test]
    fn the_json_round_trips_and_keeps_repetitions_and_medians() {
        let report = report();
        let json = serde_json::to_string_pretty(&report).expect("serialise");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["medians"][0]["rps"], 110.0);
        assert_eq!(parsed["measurements"].as_array().map(Vec::len), Some(3));
        assert_eq!(parsed["headroom"][0]["concurrency"], 1);
        assert_eq!(parsed["schema"], REPORT_SCHEMA);
    }
}
