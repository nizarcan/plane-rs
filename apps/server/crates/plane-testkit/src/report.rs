// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The run report written by `--report`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::compare::Difference;
use crate::normalize::AppliedRule;

#[derive(Debug, Clone, Default, Serialize)]
pub struct SnapshotStats {
    pub count: usize,
    pub total_ms: f64,
    pub average_ms: f64,
    pub max_ms: f64,
}

impl SnapshotStats {
    pub fn record(&mut self, millis: f64) {
        self.count += 1;
        self.total_ms += millis;
        self.max_ms = self.max_ms.max(millis);
        self.average_ms = self.total_ms / self.count as f64;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SideReport {
    pub target: String,
    pub url: String,
    pub database: String,
    pub status: u16,
    pub duration_ms: f64,
    pub snapshot_ms: f64,
    pub headers: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub body_modified: bool,
    pub formatting: String,
    pub db: serde_json::Value,
    pub celery: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub index: usize,
    pub name: String,
    pub method: String,
    pub path: String,
    pub a: SideReport,
    pub b: SideReport,
    pub differences: Vec<Difference>,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetRules {
    pub target: String,
    pub rules: Vec<AppliedRule>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub run_id: String,
    pub scenario: String,
    pub targets: BTreeMap<String, String>,
    pub a_url: String,
    pub b_url: String,
    pub a_db: String,
    pub b_db: String,
    pub started_at: String,
    pub finished_at: String,
    pub total_secs: f64,
    pub snapshot: BTreeMap<String, SnapshotStats>,
    pub normalisation: Vec<TargetRules>,
    pub steps: Vec<StepReport>,
    pub ok: bool,
}

impl RunReport {
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).context("serialising the run report")?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn failed_steps(&self) -> impl Iterator<Item = &StepReport> {
        self.steps.iter().filter(|step| !step.ok)
    }
}
