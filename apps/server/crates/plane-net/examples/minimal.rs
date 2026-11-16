// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Baseline binary for the size comparison in the crate report.
//!
//! Built with the same profile as `cost.rs` so the difference is the cost of
//! `plane-net` (reqwest + rustls + aws-lc-rs).

fn main() {
    println!("baseline {}", std::process::id());
}
