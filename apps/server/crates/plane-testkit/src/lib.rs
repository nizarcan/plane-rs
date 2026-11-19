// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-testkit`: the differential test harness for the Rust rewrite.
//!
//! The library half holds the normaliser, the comparator, the database and
//! Redis snapshotters and the run orchestration; the `difftest` binary is a
//! thin command-line shell around it.

pub mod b64;
pub mod bench;
pub mod celery;
pub mod compare;
pub mod config;
pub mod cookies;
pub mod csrf;
pub mod db;
pub mod fixture_redis;
pub mod http;
pub mod normalize;
pub mod ownership;
pub mod password;
pub mod pickle;
pub mod presign;
pub mod provision;
pub mod record;
pub mod redis;
pub mod report;
pub mod runner;
pub mod s3;
pub mod selftest;
pub mod session;
pub mod task;
pub mod task_case;
pub mod timeparse;
pub mod util;

pub use runner::{AttachOptions, RunOptions, run};
