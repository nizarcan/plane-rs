// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Bench scenario files (`scenarios/bench/*.yaml`).
//!
//! A bench scenario is a differential scenario with two changes:
//!
//! * `setup` is an ordinary [`Step`] list, exactly like a differential
//!   scenario's `steps`: same methods, bodies, cookie jars, captures and
//!   `expect_status`. It runs **once per target start**, before the settle, and
//!   its captured variables are available to every request below. That is how
//!   an authenticated read path will be benchmarked: sign in during setup,
//!   capture the workspace slug and any ids from the fixture rows, then use
//!   them in the measured requests.
//! * `requests` is a weighted list of request templates. A template is the
//!   same shape as a step (method, path, query, headers, json/form/body, jar,
//!   `expect_status`), plus `weight`; its share of the traffic is
//!   `weight / sum(weights)`.
//!
//! Everything is validated before a container, database or target is touched,
//! and the setup steps are validated by the differential scenario validator
//! itself so the two formats cannot drift.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{DEFAULT_TEMPLATE_DB, Query, RESERVED_TARGET_ENV, Scenario, Step};
use crate::util::read_yaml;

/// The Redis logical database bench runs use by default. Differential runs
/// default to 1 and 2, so bench traffic (and the cache/throttle keys a future
/// authenticated scenario writes) stays out of their way.
pub const DEFAULT_REDIS_DB: i64 = 3;

/// Largest sum of weights: the schedule is materialised as one slot per
/// weight unit, so a typo like `weight: 1000000` fails instead of allocating
/// a huge vector.
pub const MAX_TOTAL_WEIGHT: u32 = 4096;

fn default_template_db() -> String {
    DEFAULT_TEMPLATE_DB.to_string()
}

fn default_redis_db() -> i64 {
    DEFAULT_REDIS_DB
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_jar() -> String {
    "default".to_string()
}

fn default_weight() -> u32 {
    1
}

fn default_expect_status() -> u16 {
    200
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchScenario {
    pub name: String,
    #[serde(default = "default_template_db")]
    pub template_db: String,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Extra environment for the managed target. Same semantics as a
    /// differential scenario's `target_env`; per-target resource selectors are
    /// rejected.
    #[serde(default)]
    pub target_env: BTreeMap<String, String>,
    /// Redis logical database to flush before the run. Bench uses one target,
    /// so one index, unlike the differential harness's `[A, B]`.
    #[serde(default = "default_redis_db")]
    pub redis_db: i64,
    /// Requests run once per target start, before the idle measurement.
    #[serde(default)]
    pub setup: Vec<Step>,
    pub requests: Vec<BenchRequest>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchRequest {
    pub name: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<Query>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Cookie jar whose `Cookie:` header is sent with every request of this
    /// template, as filled by the setup steps. Default `default`.
    #[serde(default = "default_jar")]
    pub jar: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    /// Any other status counts as an error. Default 200.
    #[serde(default = "default_expect_status")]
    pub expect_status: u16,
}

impl BenchScenario {
    pub fn load(path: &Path) -> Result<Self> {
        let scenario: Self = read_yaml(path)?;
        scenario
            .validate()
            .with_context(|| format!("validating {}", path.display()))?;
        Ok(scenario)
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("bench scenario: `name` must not be empty");
        }
        if !(0..=15).contains(&self.redis_db) {
            bail!(
                "bench scenario {}: `redis_db` must be an integer 0-15, got {}",
                self.name,
                self.redis_db
            );
        }
        for key in self.target_env.keys() {
            if key.trim().is_empty() {
                bail!(
                    "bench scenario {}: `target_env` has an empty variable name",
                    self.name
                );
            }
            if RESERVED_TARGET_ENV.contains(&key.as_str()) {
                bail!(
                    "bench scenario {}: `target_env` may not set {key}: the harness gives the \
                     target its own database, Redis index and bind address",
                    self.name
                );
            }
        }
        // The setup steps are differential steps, so validate them with the
        // differential validator instead of a second copy of its rules.
        if !self.setup.is_empty() {
            let probe = Scenario {
                name: format!("{} (setup)", self.name),
                template_db: self.template_db.clone(),
                vars: self.vars.clone(),
                password_vars: Vec::new(),
                redis_db: vec![self.redis_db, self.redis_db],
                target_env: BTreeMap::new(),
                probe_compare_celery: false,
                steps: self.setup.clone(),
            };
            probe.validate()?;
        }
        if self.requests.is_empty() {
            bail!("bench scenario {}: no requests", self.name);
        }
        let mut names = BTreeSet::new();
        let mut total_weight: u32 = 0;
        for (index, request) in self.requests.iter().enumerate() {
            let where_ = format!(
                "bench scenario {} request {} ({})",
                self.name,
                index + 1,
                request.name
            );
            if request.name.trim().is_empty() {
                bail!("{where_}: request name must not be empty");
            }
            if !names.insert(request.name.clone()) {
                bail!("{where_}: duplicate request name");
            }
            if request.path.trim().is_empty() || !request.path.starts_with('/') {
                bail!("{where_}: `path` must start with `/`");
            }
            if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
                .contains(&request.method.as_str())
            {
                bail!("{where_}: unsupported method {:?}", request.method);
            }
            let bodies = usize::from(request.json.is_some())
                + usize::from(request.form.is_some())
                + usize::from(request.body.is_some());
            if bodies > 1 {
                bail!("{where_}: at most one of `json`, `form`, `body` may be set");
            }
            if request.weight == 0 {
                bail!("{where_}: `weight` must be at least 1");
            }
            total_weight = total_weight.saturating_add(request.weight);
            if total_weight > MAX_TOTAL_WEIGHT {
                bail!("{where_}: the summed weights exceed {MAX_TOTAL_WEIGHT}");
            }
            if !(100..=599).contains(&request.expect_status) {
                bail!(
                    "{where_}: `expect_status` must be an HTTP status code, got {}",
                    request.expect_status
                );
            }
        }
        Ok(())
    }

    /// One slot per weight unit, holding the index of the request template it
    /// belongs to. A worker picks a slot with its own small PRNG, so the mix
    /// is exact over a run and the workers do not march in lockstep.
    pub fn weighted_schedule(&self) -> Vec<usize> {
        let mut schedule = Vec::with_capacity(
            self.requests
                .iter()
                .map(|request| request.weight as usize)
                .sum(),
        );
        for (index, request) in self.requests.iter().enumerate() {
            for _ in 0..request.weight {
                schedule.push(index);
            }
        }
        schedule
    }

    /// The reserved `fixture_sql` variable, resolved against the repository
    /// root. Same rule as `config::Scenario::fixture_sql` (keep the two in
    /// step): bench applies the SQL to the managed clone before setup.
    pub fn fixture_sql(&self) -> Option<std::path::PathBuf> {
        self.vars.get(crate::config::FIXTURE_SQL_VAR).map(|path| {
            std::path::PathBuf::from(
                path.replace("{repo}", &crate::util::repo_root().display().to_string()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CaptureSource;

    fn parse(text: &str) -> BenchScenario {
        serde_yaml_ng::from_str(text).expect("parse")
    }

    #[test]
    fn a_minimal_scenario_applies_the_documented_defaults() {
        let scenario = parse("name: t\nrequests:\n  - name: r\n    path: /\n");
        scenario.validate().expect("valid");
        assert_eq!(scenario.template_db, DEFAULT_TEMPLATE_DB);
        assert_eq!(scenario.redis_db, DEFAULT_REDIS_DB);
        let request = &scenario.requests[0];
        assert_eq!(request.method, "GET");
        assert_eq!(request.jar, "default");
        assert_eq!(request.weight, 1);
        assert_eq!(request.expect_status, 200);
    }

    #[test]
    fn request_bodies_follow_the_differential_step_rules() {
        let scenario =
            parse("name: t\nrequests:\n  - name: r\n    path: /\n    json: {a: 1}\n    body: x\n");
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("at most one"));
    }

    #[test]
    fn unknown_request_fields_are_rejected() {
        let error = serde_yaml_ng::from_str::<BenchScenario>(
            "name: t\nrequests:\n  - name: r\n    path: /\n    weigth: 2\n",
        )
        .expect_err("must reject");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn duplicate_names_and_zero_weights_are_rejected() {
        let duplicate =
            parse("name: t\nrequests:\n  - name: r\n    path: /\n  - name: r\n    path: /x\n");
        assert!(
            duplicate
                .validate()
                .expect_err("duplicate")
                .to_string()
                .contains("duplicate request name")
        );

        let zero = parse("name: t\nrequests:\n  - name: r\n    path: /\n    weight: 0\n");
        assert!(
            zero.validate()
                .expect_err("zero weight")
                .to_string()
                .contains("at least 1")
        );
    }

    #[test]
    fn a_runaway_weight_is_rejected_before_allocating_the_schedule() {
        let scenario = parse(
            "name: t\nrequests:\n  - name: r\n    path: /\n    weight: 4096\n  - name: s\n    path: /x\n",
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("summed weights"));
    }

    #[test]
    fn the_schedule_has_one_slot_per_weight_unit() {
        let scenario = parse(
            "name: t\nrequests:\n  - name: a\n    path: /\n    weight: 3\n  - name: b\n    path: /b\n    weight: 1\n",
        );
        scenario.validate().expect("valid");
        let schedule = scenario.weighted_schedule();
        assert_eq!(schedule, vec![0, 0, 0, 1]);
    }

    #[test]
    fn setup_is_validated_as_a_differential_step_list() {
        let scenario = parse(
            "name: t\nsetup:\n  - name: sign-in\n    path: /auth/sign-in/\n    capture:\n      - name: csrf\n        source: json\nrequests:\n  - name: r\n    path: /\n",
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs `pointer`"), "{error}");
    }

    #[test]
    fn setup_captures_are_part_of_the_validated_shape() {
        let scenario = parse(
            r#"
name: t
setup:
  - name: sign-in
    method: POST
    path: /auth/sign-in/
    form: {email: a@b.c}
    capture:
      - name: csrf
        source: cookie
        cookie: csrftoken
requests:
  - name: r
    path: /api/users/me/
    jar: user
"#,
        );
        scenario.validate().expect("valid");
        assert_eq!(scenario.setup[0].capture[0].source, CaptureSource::Cookie);
    }

    #[test]
    fn target_env_may_not_select_a_per_target_resource() {
        let scenario =
            parse("name: t\ntarget_env:\n  PLANE_DB: x\nrequests:\n  - name: r\n    path: /\n");
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("its own database"));
    }

    #[test]
    fn a_redis_index_outside_the_server_range_is_rejected() {
        let scenario = parse("name: t\nredis_db: 16\nrequests:\n  - name: r\n    path: /\n");
        assert!(
            scenario
                .validate()
                .expect_err("must reject")
                .to_string()
                .contains("0-15")
        );
    }

    #[test]
    fn expect_status_accepts_a_non_2xx_template() {
        let scenario = parse(
            "name: t\nrequests:\n  - name: missing\n    path: /nope\n    expect_status: 404\n",
        );
        scenario
            .validate()
            .expect("404 is a legitimate expectation");
    }

    #[test]
    fn a_fixture_sql_variable_is_resolved_against_the_repository_root() {
        let scenario = parse(
            "name: t\nvars:\n  fixture_sql: \"{repo}/scenarios/x.sql\"\nrequests:\n  - name: r\n    path: /\n",
        );
        let path = scenario.fixture_sql().expect("fixture path");
        assert!(path.is_absolute(), "{}", path.display());
        assert!(path.ends_with("scenarios/x.sql"), "{}", path.display());
        // A scenario without the variable has no fixture.
        let none = parse("name: t\nrequests:\n  - name: r\n    path: /\n");
        assert!(none.fixture_sql().is_none());
    }

    /// The committed scenarios are the ones BENCH.md quotes numbers from: a
    /// typo in one must fail here, not thirty minutes into a matrix run.
    #[test]
    fn the_committed_bench_scenarios_load_and_validate() {
        let directory = crate::util::repo_root().join("apps/server/scenarios/bench");
        let mut seen = 0;
        for entry in std::fs::read_dir(&directory).expect("scenarios/bench must exist") {
            let path = entry.expect("directory entry").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("yaml") {
                continue;
            }
            let scenario = BenchScenario::load(&path)
                .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
            assert!(!scenario.requests.is_empty());
            seen += 1;
        }
        assert!(
            seen >= 2,
            "expected at least health.yaml and static-responses.yaml, found {seen}"
        );
    }
}
