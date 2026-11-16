// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python's `repr(float)`, for the cursor's `value` and for JSON numbers.
//!
//! `plane-serde` owns the JSON number formatting (`PyFloatRepr`), but it is not
//! public; the cursor only needs values in `0.0..=1000.0`, where Python's repr
//! is `str(value)` with a `.0` on integers. That is implemented here rather
//! than re-exporting an internal.

/// `repr(float)` for a finite value in the range cursors use.
pub fn repr(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_floats_keep_the_point_zero() {
        assert_eq!(repr(10.0), "10.0");
        assert_eq!(repr(13.0), "13.0");
        assert_eq!(repr(0.0), "0.0");
        assert_eq!(repr(13.5), "13.5");
    }
}
