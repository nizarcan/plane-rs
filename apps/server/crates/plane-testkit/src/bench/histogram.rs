// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Latency histograms and the small statistics helpers the bench report uses.
//!
//! Latencies are recorded in microseconds in an HDR histogram
//! (`hdrhistogram`), so a p99 is a bucket lookup rather than a sort of every
//! sample, and the memory cost does not grow with the number of requests: a
//! 20 s run at 100k req/s keeps the same ~2 MiB histogram as a 20-request run.
//! The bounds are 1 µs to 60 s with three significant figures, which is more
//! resolution than the client can measure and covers every timeout the
//! harness allows.
//!
//! A run of N repetitions is summarised by the **median** of the per-run
//! values, not by pooling the samples: pooling three runs would hide a single
//! slow run behind two fast ones, and the brief asks for medians.

use std::time::Duration;

use hdrhistogram::Histogram;
use serde::Serialize;

/// Highest latency a histogram can hold: 60 s, in microseconds.
pub const MAX_MICROS: u64 = 60_000_000;

/// Significant figures kept per bucket. At three, every value below 2048 µs
/// has its own bucket (the resolution is 1 µs where it matters).
pub const SIGNIFICANT_FIGURES: u8 = 3;

/// A latency distribution. Cheap to merge: one histogram per worker thread,
/// merged once when the worker finishes.
#[derive(Clone)]
pub struct Latencies {
    histogram: Histogram<u64>,
}

impl Default for Latencies {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Latencies {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let summary = self.summary();
        formatter
            .debug_struct("Latencies")
            .field("count", &summary.count)
            .field("p50_ms", &summary.p50_ms)
            .field("p99_ms", &summary.p99_ms)
            .finish()
    }
}

impl Latencies {
    pub fn new() -> Self {
        // The bounds are compile-time constants, so this cannot fail.
        let histogram = Histogram::new_with_bounds(1, MAX_MICROS, SIGNIFICANT_FIGURES)
            .expect("static histogram bounds are valid");
        Self { histogram }
    }

    pub fn record_micros(&mut self, micros: u64) {
        // A 0 µs sample (below the low bound) is clamped to 1 µs, and a
        // latency above 60 s is clamped to the ceiling rather than dropped: a
        // request that took that long is a result, not a reason to fail.
        self.histogram.saturating_record(micros.max(1));
    }

    pub fn record(&mut self, latency: Duration) {
        self.record_micros(u64::try_from(latency.as_micros()).unwrap_or(MAX_MICROS));
    }

    /// Merge `other` into this histogram. Both use the same static bounds, so
    /// the only error `add` can report is a mismatch in the bounds, which is a
    /// programming error rather than a runtime condition.
    pub fn merge(&mut self, other: &Latencies) {
        if self.histogram.add(&other.histogram).is_err() {
            debug_assert!(false, "histograms with the same bounds must merge");
        }
    }

    pub fn len(&self) -> u64 {
        self.histogram.len()
    }

    pub fn is_empty(&self) -> bool {
        self.histogram.is_empty()
    }

    pub fn max_micros(&self) -> u64 {
        if self.histogram.is_empty() {
            0
        } else {
            self.histogram.max()
        }
    }

    /// The value at `quantile` (0.0–1.0), using HDR's "highest equivalent
    /// value" convention so the reported percentile is never below the true
    /// value.
    pub fn value_at_quantile_micros(&self, quantile: f64) -> u64 {
        if self.histogram.is_empty() {
            0
        } else {
            self.histogram.value_at_quantile(quantile)
        }
    }

    pub fn summary(&self) -> LatencySummary {
        let micros_to_ms = |micros: u64| micros as f64 / 1000.0;
        LatencySummary {
            count: self.histogram.len(),
            mean_ms: if self.histogram.is_empty() {
                0.0
            } else {
                self.histogram.mean() / 1000.0
            },
            p50_ms: micros_to_ms(self.value_at_quantile_micros(0.50)),
            p90_ms: micros_to_ms(self.value_at_quantile_micros(0.90)),
            p95_ms: micros_to_ms(self.value_at_quantile_micros(0.95)),
            p99_ms: micros_to_ms(self.value_at_quantile_micros(0.99)),
            max_ms: micros_to_ms(self.max_micros()),
        }
    }
}

/// The percentiles the brief asks for, plus the count and mean so a row can be
/// read without the histogram. All values are milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct LatencySummary {
    pub count: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

/// Median of a list, or `None` when it is empty. For an even count this is the
/// mean of the two middle values, the usual definition; the bench uses an odd
/// repetition count (three) so the median is always an observed run.
pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        Some(sorted[middle])
    } else {
        Some((sorted[middle - 1] + sorted[middle]) / 2.0)
    }
}

/// Median of integer counts (requests, RSS bytes) as an integer.
pub fn median_u64(values: &[u64]) -> Option<u64> {
    median(&values.iter().map(|value| *value as f64).collect::<Vec<_>>())
        .map(|value| value.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn microseconds_1_to(count: u64) -> Latencies {
        let mut latencies = Latencies::new();
        for value in 1..=count {
            latencies.record_micros(value);
        }
        latencies
    }

    #[test]
    fn percentiles_of_a_known_distribution_are_exact() {
        // The values below 2048 µs each get their own bucket at three
        // significant figures, so the percentiles are the observed values.
        let summary = microseconds_1_to(1000).summary();
        assert_eq!(summary.count, 1000);
        assert!((summary.p50_ms - 0.500).abs() < 1e-9, "{summary:?}");
        assert!((summary.p90_ms - 0.900).abs() < 1e-9, "{summary:?}");
        assert!((summary.p95_ms - 0.950).abs() < 1e-9, "{summary:?}");
        assert!((summary.p99_ms - 0.990).abs() < 1e-9, "{summary:?}");
        assert!((summary.max_ms - 1.000).abs() < 1e-9, "{summary:?}");
        assert!((summary.mean_ms - 0.5005).abs() < 1e-4, "{summary:?}");
    }

    #[test]
    fn percentiles_never_understate_the_observed_value() {
        let mut latencies = Latencies::new();
        latencies.record(Duration::from_millis(7));
        latencies.record(Duration::from_millis(9));
        let summary = latencies.summary();
        assert!(summary.p50_ms >= 7.0);
        assert!(summary.p99_ms >= 9.0);
        // HDR reports the highest equivalent value in the sample's bucket, so
        // a 9 ms observation can read as 9.007 ms; it never reads lower.
        assert!((9.0..9.01).contains(&summary.max_ms), "{summary:?}");
    }

    #[test]
    fn merging_is_associative_and_preserves_the_distribution() {
        let whole = microseconds_1_to(1000);
        let mut halves = microseconds_1_to(500);
        let mut upper = Latencies::new();
        for value in 501..=1000 {
            upper.record_micros(value);
        }
        halves.merge(&upper);
        assert_eq!(halves.len(), whole.len());
        assert_eq!(halves.summary(), whole.summary());
    }

    #[test]
    fn an_empty_histogram_reports_zeros_rather_than_nan() {
        let summary = Latencies::new().summary();
        assert_eq!(summary, LatencySummary::default());
    }

    #[test]
    fn a_latency_beyond_the_ceiling_is_clamped_not_panicked() {
        let mut latencies = Latencies::new();
        latencies.record(Duration::from_secs(3600));
        // Clamped to the highest bucket of the 60 s range, which is the
        // ceiling rounded up to the bucket boundary.
        assert!(
            latencies.max_micros() >= MAX_MICROS,
            "{}",
            latencies.max_micros()
        );
        assert!(
            latencies.max_micros() < MAX_MICROS + 100_000,
            "{}",
            latencies.max_micros()
        );
        assert_eq!(latencies.len(), 1);
    }

    #[test]
    fn median_uses_the_middle_value_for_an_odd_count() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[10.0]), Some(10.0));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn median_averages_the_two_middle_values_for_an_even_count() {
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(median_u64(&[1, 2, 3, 4]), Some(3));
        assert_eq!(median_u64(&[7, 7, 9]), Some(7));
        assert_eq!(median_u64(&[]), None);
    }
}
