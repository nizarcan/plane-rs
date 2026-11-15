// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-testkit`: the differential test harness for the Rust rewrite.
//!
//! The library half holds the normaliser, the comparator, the database and
//! Redis snapshotters and the run orchestration; the `difftest` binary is a
//! thin command-line shell around it.

pub mod b64;
pub mod celery;
pub mod compare;
pub mod config;
pub mod cookies;
pub mod csrf;
pub mod db;
pub mod http;
pub mod normalize;
pub mod password;
pub mod pickle;
pub mod provision;
pub mod record;
pub mod redis;
pub mod report;
pub mod runner;
pub mod selftest;
pub mod session;
pub mod timeparse;
pub mod util;

pub use runner::{AttachOptions, RunOptions, run};
