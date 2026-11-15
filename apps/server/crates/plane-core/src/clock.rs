// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Time, behind one function so tests can reason about it.

use chrono::{DateTime, Utc};

/// The current UTC instant. Django's `USE_TZ=True` and `TIME_ZONE="UTC"` mean
/// every stored timestamp is UTC (`ENV.md` §2.9).
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// Whole milliseconds between two instants, saturating at zero, for the
/// `duration_ms` field of the request log (HTTP_CONTRACT §9.3).
pub fn millis_between(start: std::time::Instant, end: std::time::Instant) -> u64 {
    end.saturating_duration_since(start).as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millis_are_saturating() {
        let start = std::time::Instant::now();
        assert_eq!(millis_between(start, start), 0);
        let later = start + std::time::Duration::from_millis(7);
        assert_eq!(millis_between(start, later), 7);
        assert_eq!(millis_between(later, start), 0);
    }
}
