// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Resident memory and CPU time for a whole target process tree.
//!
//! A managed target is started in its own process group (`provision::
//! TargetInstance`), so its PID is also its PGID and every process it forks —
//! the gunicorn master, each uvicorn worker, any child a request spawns — keeps
//! that PGID. Sampling the group therefore measures the tree, not just the
//! process the harness happened to spawn.
//!
//! The data comes from `ps -A -o pgid=,pid=,rss=,time=`, which is the
//! portable way to get both numbers:
//!
//! * `rss` is in KiB on macOS and Linux and is summed over the tree. That
//!   deliberately over-counts shared pages (gunicorn workers share the parent's
//!   copy-on-write pages, and every process maps the same libraries), so the
//!   number is a conservative upper bound, not a PSS. `BENCH.md` says so.
//! * `time` is consumed CPU time (user + system) at centisecond resolution on
//!   macOS, which is ample for a 20 s window. Taking the difference between the
//!   first and last sample gives the CPU seconds of the measured window.
//!
//! The harness itself is not in the target's process group, so `ps` and the
//! harness's own work never appear in the numbers.

use std::io;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;

/// One `ps` row, reduced to what the bench needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessRow {
    pub pgid: u32,
    pub pid: u32,
    pub rss_kib: u64,
    pub cpu_secs: f64,
}

/// Summed RSS and CPU time of a process tree at one instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ProcessSample {
    pub rss_bytes: u64,
    pub cpu_secs: f64,
    pub processes: u64,
}

impl ProcessSample {
    fn add(&mut self, row: &ProcessRow) {
        self.rss_bytes += row.rss_kib * 1024;
        self.cpu_secs += row.cpu_secs;
        self.processes += 1;
    }
}

/// What a [`TreeSampler`] observed over one measured window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SamplerSummary {
    /// Highest summed RSS seen by the 250 ms sampler or by the final sample.
    pub peak_rss_bytes: u64,
    /// Summed RSS when the sampler stopped, before the target is restarted.
    pub after_rss_bytes: u64,
    /// Consumed CPU seconds (user + system) over the measured window.
    pub cpu_secs: f64,
    /// Number of 250 ms samples taken (excludes the final one).
    pub samples: u64,
    /// Highest number of processes in the tree at any sample.
    pub processes_peak: u64,
    /// True when the final sample found no process in the group, which means
    /// the target exited during the window and both `after_rss_bytes` and
    /// `cpu_secs` under-report (a dead process's CPU time disappears from `ps`).
    pub process_missing: bool,
}

/// Parse the `ps -A -o pgid=,pid=,rss=,time=` table.
pub fn parse_ps_table(text: &str) -> Vec<ProcessRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 4 {
            continue;
        }
        let (Ok(pgid), Ok(pid), Ok(rss_kib)) = (
            columns[0].parse::<u32>(),
            columns[1].parse::<u32>(),
            columns[2].parse::<u64>(),
        ) else {
            continue;
        };
        let Some(cpu_secs) = parse_ps_time(columns[3]) else {
            continue;
        };
        rows.push(ProcessRow {
            pgid,
            pid,
            rss_kib,
            cpu_secs,
        });
    }
    rows
}

/// Parse `ps`'s `time` column: `MM:SS.cc`, `HH:MM:SS.cc` or `D-HH:MM:SS.cc`.
pub fn parse_ps_time(text: &str) -> Option<f64> {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (
            0,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        _ => return None,
    };
    if minutes >= 60 || seconds >= 60.0 {
        return None;
    }
    Some(days as f64 * 86_400.0 + hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

/// Sample one process group with `ps`. An empty group is not an error: it
/// means the target has exited, which the caller reports as `process_missing`.
pub fn sample_process_tree(pgid: u32) -> io::Result<ProcessSample> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pgid=,pid=,rss=,time="])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "ps exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut sample = ProcessSample::default();
    for row in parse_ps_table(&text) {
        if row.pgid == pgid {
            sample.add(&row);
        }
    }
    Ok(sample)
}

#[derive(Debug, Clone)]
struct State {
    peak_rss_bytes: u64,
    processes_peak: u64,
    samples: u64,
    last: ProcessSample,
}

/// Background sampler for one measured window. Sampling runs on a plain
/// thread: `ps` is a blocking subprocess and must never block the async
/// runtime that is driving the load.
pub struct TreeSampler {
    pgid: u32,
    interval: Duration,
    baseline: ProcessSample,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TreeSampler {
    /// Take the baseline sample and start sampling every `interval`.
    pub fn start(pgid: u32, interval: Duration) -> io::Result<Self> {
        let baseline = sample_process_tree(pgid)?;
        let state = Arc::new(Mutex::new(State {
            peak_rss_bytes: baseline.rss_bytes,
            processes_peak: baseline.processes,
            samples: 0,
            last: baseline,
        }));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("bench-ps".to_string())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    if let Ok(sample) = sample_process_tree(pgid)
                        && let Ok(mut state) = thread_state.lock()
                    {
                        state.samples += 1;
                        state.last = sample;
                        state.peak_rss_bytes = state.peak_rss_bytes.max(sample.rss_bytes);
                        state.processes_peak = state.processes_peak.max(sample.processes);
                    }
                    // Slept in slices so `finish` returns promptly (the load
                    // window ends on a deadline, and a 250 ms tail on every
                    // measurement would add up over 54 measurements).
                    let deadline = std::time::Instant::now() + interval;
                    while std::time::Instant::now() < deadline {
                        if thread_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            })
            .map_err(io::Error::other)?;

        Ok(Self {
            pgid,
            interval,
            baseline,
            state,
            stop,
            handle: Some(handle),
        })
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// The last sample the background thread took.
    pub fn last(&self) -> ProcessSample {
        self.state
            .lock()
            .map(|state| state.last)
            .unwrap_or_default()
    }

    /// Stop sampling and produce the summary. The final `ps` sample is taken
    /// after the thread is joined so it reflects the post-load tree.
    pub fn finish(mut self) -> io::Result<SamplerSummary> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let after = sample_process_tree(self.pgid)?;
        let state = self
            .state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
        let peak_rss_bytes = state.peak_rss_bytes.max(after.rss_bytes);
        Ok(SamplerSummary {
            peak_rss_bytes,
            after_rss_bytes: after.rss_bytes,
            cpu_secs: (after.cpu_secs - self.baseline.cpu_secs).max(0.0),
            samples: state.samples,
            processes_peak: state.processes_peak.max(after.processes),
            process_missing: after.processes == 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_times_are_parsed_in_every_format() {
        assert_eq!(parse_ps_time("0:00.01"), Some(0.01));
        assert_eq!(parse_ps_time("1:02.50"), Some(62.5));
        assert_eq!(parse_ps_time("13:41.84"), Some(821.84));
        assert_eq!(parse_ps_time("2:03:04.25"), Some(7384.25));
        assert_eq!(parse_ps_time("1-02:03:04.50"), Some(93_784.5));
        // A value that is not a time is skipped rather than guessed at.
        assert_eq!(parse_ps_time("n/a"), None);
        assert_eq!(parse_ps_time("1:99.00"), None);
        assert_eq!(parse_ps_time(""), None);
    }

    #[test]
    fn the_ps_table_is_summed_per_process_group() {
        let table = "  349   349  38768  9:19.31\n\
                       349   351  20000  0:20.62\n\
                       400   401   1024  0:00.10\n\
                     garbage line\n\
                       349   352    512  0:00.05\n";
        let rows = parse_ps_table(table);
        assert_eq!(rows.len(), 4);
        let mut sample = ProcessSample::default();
        for row in rows.iter().filter(|row| row.pgid == 349) {
            sample.add(row);
        }
        assert_eq!(sample.processes, 3);
        assert_eq!(sample.rss_bytes, (38_768 + 20_000 + 512) * 1024);
        assert!((sample.cpu_secs - (559.31 + 20.62 + 0.05)).abs() < 1e-9);
    }

    #[test]
    fn an_empty_process_group_is_an_empty_sample() {
        // PID 0 is the kernel and PID 1 is launchd; neither is a group the
        // harness can start, so pick a group id that cannot exist.
        let sample = sample_process_tree(u32::MAX).expect("ps must run");
        assert_eq!(sample, ProcessSample::default());
    }

    /// Helper run in a child process by `a_child_that_allocates_is_measured`.
    /// It does nothing in a normal `cargo test` run.
    #[test]
    fn child_helper_allocates() {
        if std::env::var("DIFFTEST_BENCH_CHILD").as_deref() != Ok("allocate") {
            return;
        }
        // Touch 96 MiB so it is resident rather than lazily mapped, then burn
        // CPU for long enough that `ps`'s centisecond clock sees it.
        let mut memory = vec![0_u8; 96 * 1024 * 1024];
        for offset in (0..memory.len()).step_by(4096) {
            memory[offset] = 1;
        }
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_millis(300) {
            std::hint::black_box(&memory);
        }
        std::thread::sleep(Duration::from_millis(1500));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_allocates_is_measured() {
        use std::os::unix::process::CommandExt;

        let executable = std::env::current_exe().expect("current test binary");
        let mut child = Command::new(executable)
            .args([
                "--exact",
                "bench::process::tests::child_helper_allocates",
                "--nocapture",
            ])
            .env("DIFFTEST_BENCH_CHILD", "allocate")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawning the helper child");
        let pgid = child.id();

        // Wait until the child is resident: 96 MiB allocated and touched. A
        // cold `cargo test` start can take a moment to reach the test.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut max_seen = 0;
        while std::time::Instant::now() < deadline {
            if let Ok(sample) = sample_process_tree(pgid) {
                max_seen = max_seen.max(sample.rss_bytes);
                if sample.rss_bytes > 64 * 1024 * 1024 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            max_seen > 64 * 1024 * 1024,
            "the helper's RSS never reached 64 MiB (peak sample {max_seen} bytes)"
        );

        let sampler = TreeSampler::start(pgid, Duration::from_millis(100)).expect("sampler");
        std::thread::sleep(Duration::from_millis(450));
        let summary = sampler.finish().expect("summary");
        assert!(
            summary.peak_rss_bytes >= 64 * 1024 * 1024,
            "peak RSS {} bytes is below the 64 MiB the helper touched",
            summary.peak_rss_bytes
        );
        assert!(
            summary.samples >= 2,
            "expected several samples in 450 ms, got {}",
            summary.samples
        );
        assert!(
            summary.cpu_secs > 0.05,
            "the helper burned 300 ms of CPU; `ps` reported {} s",
            summary.cpu_secs
        );
        assert!(!summary.process_missing);

        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{pgid}"))
            .status();
        let _ = child.wait();
    }
}
