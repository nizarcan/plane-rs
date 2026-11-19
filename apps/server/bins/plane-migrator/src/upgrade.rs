// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Ordered schema steps that carry an existing database from one upstream pin
//! to the next (`UPGRADING.md`, "Schema changes").
//!
//! `deploy/schema/baseline.sql` is regenerated at every pin, so a fresh install
//! always gets the newest schema. A database created at an older pin is moved
//! forward by the steps below, in order, each one the output of Django's own
//! `manage.py sqlmigrate <app> <migration>` at the new commit:
//!
//! ```text
//! UpgradeStep {
//!     id: "0001",
//!     from: "db:0125_reset_project_deploy_board_identity,license:0006_…",
//!     to:   "db:0126_add_widget,license:0006_…",
//!     sql:  include_str!("../../../deploy/schema/upgrade_steps/0001_db_0126_add_widget.sql"),
//!     migrations: &[("db", "0126_add_widget")],
//! }
//! ```
//!
//! The migrator wraps each step in one transaction, executes the SQL and
//! inserts the `django_migrations` rows the step stands for, then moves the
//! `plane_jobs.meta.django_baseline` marker to `to`. `sqlmigrate` emits nothing
//! for a `RunPython` migration: such a step needs a hand-written SQL backfill
//! (reviewed against the Python body) and the same `migrations` rows; see
//! `UPGRADING.md`.
//!
//! A `from` that matches no step is refused, never guessed: that is a database
//! from an unknown release, older or newer than this binary.

/// One post-baseline schema step.
pub struct UpgradeStep {
    /// Stable identifier, also the SQL file's prefix (`0001`, `0002`, ...).
    pub id: &'static str,
    /// The `plane_jobs.meta.django_baseline` value the step applies to.
    pub from: &'static str,
    /// The value written after the step succeeds; the next step's `from`.
    pub to: &'static str,
    /// `manage.py sqlmigrate` output, without its transaction wrapper.
    pub sql: &'static str,
    /// The `(app, name)` rows this step records in `django_migrations`.
    pub migrations: &'static [(&'static str, &'static str)],
}

/// Every step released so far, oldest first. Empty while the pin has not moved.
pub const UPGRADE_STEPS: &[UpgradeStep] = &[];
