// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Typed views of the three YAML files the harness reads: `targets.yaml`,
//! `normalize.yaml` and scenario files.
//!
//! Everything is validated up front so a typo in a scenario fails before any
//! container, database or target is touched.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::util::read_yaml;

pub const DEFAULT_TEMPLATE_DB: &str = "plane_tpl";

fn default_template_db() -> String {
    DEFAULT_TEMPLATE_DB.to_string()
}

fn default_health_timeout() -> u64 {
    60
}

fn default_redis_db() -> Vec<i64> {
    vec![1, 2]
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_jar() -> String {
    "default".to_string()
}

fn default_port_range() -> Vec<u16> {
    vec![18000, 18099]
}

fn default_timestamp_slack() -> i64 {
    2
}

fn default_db_schemas() -> Vec<String> {
    vec!["public".to_string()]
}

fn default_ignore_headers() -> Vec<String> {
    // The brief fixes these three. `date` is wall-clock, `server` names the
    // implementation (gunicorn vs hyper), `content-length` changes whenever a
    // volatile value of a different length was substituted; it is still
    // compared when no normalisation was needed (see the comparator).
    vec![
        "date".to_string(),
        "server".to_string(),
        "content-length".to_string(),
    ]
}

// ---------------------------------------------------------------------------
// targets.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetsFile {
    pub targets: BTreeMap<String, TargetSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    /// `django` today, `rust` once `plane-api` can serve the same scenario.
    pub kind: String,
    /// Inclusive port range the harness may allocate from.
    #[serde(default = "default_port_range")]
    pub port_range: Vec<u16>,
    /// Command template; the first element is relative to the repository root.
    pub command: Vec<String>,
    /// Environment template. Placeholders: `{port}`, `{db}`, `{redis_db}`,
    /// `{vhost}`, `{repo}`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Path polled until it answers with a 2xx before the scenario starts.
    pub health: String,
    #[serde(default = "default_health_timeout")]
    pub health_timeout_secs: u64,
    /// Working directory template. Defaults to the repository root.
    #[serde(default)]
    pub workdir: Option<String>,
}

impl TargetSpec {
    fn validate(&self, name: &str) -> Result<()> {
        if self.kind.trim().is_empty() {
            bail!("target {name}: `kind` must not be empty");
        }
        if self.port_range.len() != 2 || self.port_range[0] > self.port_range[1] {
            bail!("target {name}: `port_range` must be [first, last] with first <= last");
        }
        if self.command.is_empty() {
            bail!("target {name}: `command` must not be empty");
        }
        if self.health.is_empty() {
            bail!("target {name}: `health` must not be empty");
        }
        Ok(())
    }
}

impl TargetsFile {
    pub fn load(path: &Path) -> Result<Self> {
        let file: Self = read_yaml(path)?;
        for (name, spec) in &file.targets {
            spec.validate(name)?;
        }
        Ok(file)
    }

    pub fn get(&self, name: &str) -> Result<&TargetSpec> {
        let Some(spec) = self.targets.get(name) else {
            bail!(
                "unknown target {name:?}; targets.yaml defines: {}",
                self.targets.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        };
        Ok(spec)
    }
}

// ---------------------------------------------------------------------------
// Scenario files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(default = "default_template_db")]
    pub template_db: String,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Redis database indexes used when the harness starts the targets:
    /// [A, B]. Defaults to [1, 2].
    #[serde(default = "default_redis_db")]
    pub redis_db: Vec<i64>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: Option<Query>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub json: Option<serde_json::Value>,
    #[serde(default)]
    pub form: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default = "default_jar")]
    pub jar: String,
    #[serde(default)]
    pub capture: Vec<Capture>,
    /// Sanity check applied to both targets. A mismatch is reported like any
    /// other difference, so an unexpected status cannot pass unnoticed.
    #[serde(default)]
    pub expect_status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Query {
    Map(BTreeMap<String, String>),
    Raw(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub name: String,
    pub source: CaptureSource,
    #[serde(default)]
    pub pointer: Option<String>,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    /// JSON pointer into the response body.
    Json,
    /// Whole value of a response header (first occurrence).
    Header,
    /// Value of a cookie in the step's jar after the response.
    Cookie,
    /// First capture group of a regex over a response header.
    Regex,
}

impl Scenario {
    pub fn load(path: &Path) -> Result<Self> {
        let scenario: Self = read_yaml(path)?;
        scenario.validate()?;
        Ok(scenario)
    }

    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("scenario: `name` must not be empty");
        }
        if self.redis_db.len() != 2 {
            bail!("scenario: `redis_db` must contain exactly two indexes");
        }
        if self.steps.is_empty() {
            bail!("scenario {}: no steps", self.name);
        }
        let mut seen = std::collections::BTreeSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            let where_ = format!("scenario {} step {} ({})", self.name, index + 1, step.name);
            if step.name.trim().is_empty() {
                bail!("{where_}: step name must not be empty");
            }
            if !seen.insert(step.name.clone()) {
                bail!("{where_}: duplicate step name");
            }
            if step.path.trim().is_empty() || !step.path.starts_with('/') {
                bail!("{where_}: `path` must start with `/`");
            }
            if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
                .contains(&step.method.as_str())
            {
                bail!("{where_}: unsupported method {:?}", step.method);
            }
            let bodies = usize::from(step.json.is_some())
                + usize::from(step.form.is_some())
                + usize::from(step.body.is_some());
            if bodies > 1 {
                bail!("{where_}: at most one of `json`, `form`, `body` may be set");
            }
            for capture in &step.capture {
                capture.validate(&where_)?;
                if !seen.insert(format!("var:{}", capture.name)) {
                    bail!("{where_}: duplicate capture name {:?}", capture.name);
                }
            }
        }
        Ok(())
    }
}

impl Capture {
    fn validate(&self, where_: &str) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("{where_}: capture `name` must not be empty");
        }
        match self.source {
            CaptureSource::Json => {
                if self.pointer.is_none() {
                    bail!("{where_}: capture {} needs `pointer`", self.name);
                }
            }
            CaptureSource::Header => {
                if self.header.is_none() {
                    bail!("{where_}: capture {} needs `header`", self.name);
                }
            }
            CaptureSource::Cookie => {
                if self.cookie.is_none() {
                    bail!("{where_}: capture {} needs `cookie`", self.name);
                }
            }
            CaptureSource::Regex => {
                if self.header.is_none() || self.pattern.is_none() {
                    bail!(
                        "{where_}: capture {} needs `header` and `pattern`",
                        self.name
                    );
                }
                let pattern = self.pattern.as_deref().unwrap_or_default();
                regex::Regex::new(pattern).map_err(|error| {
                    anyhow::anyhow!("{where_}: invalid regex {pattern:?}: {error}")
                })?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// normalize.yaml
// ---------------------------------------------------------------------------

/// A named normalisation rule. `why` is mandatory and is copied into the run
/// report, so every exemption carries its justification with it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedRule {
    pub name: String,
    pub why: String,
    #[serde(default)]
    pub mode: ValueMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbRule {
    pub table: String,
    pub column: String,
    pub why: String,
    #[serde(default)]
    pub mode: ValueMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueMode {
    /// Replace the value with `<volatile>`.
    #[default]
    Volatile,
    /// Replace a non-empty value with `<volatile>`, keep an empty value as is.
    VolatileIfNonEmpty,
    /// Keep the `algorithm$iterations$` prefix, replace salt and hash.
    PasswordPrefix,
    /// A random `#rrggbb` colour: replace with `<colour>` only when the value
    /// has that shape, so a malformed or constant colour still shows up.
    RandomColor,
    /// A timestamp relative to the step clock: replace with
    /// `<offset+Ns>` (rounded to 10 s) so `now + a fixed age` is compared by
    /// its age rather than by its absolute value.
    RelativeTime,
    /// A Unix timestamp in seconds (for example `epoch=int(timezone.now()
    /// .timestamp())` in Celery kwargs): replace with `<epoch>` when it is
    /// inside the step window's neighbourhood, so a run that crosses a second
    /// boundary does not differ.
    UnixTime,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizeRules {
    #[serde(default = "default_ignore_headers")]
    pub ignore_headers: Vec<String>,
    #[serde(default)]
    pub volatile_cookies: Vec<NamedRule>,
    #[serde(default)]
    pub volatile_json_keys: Vec<NamedRule>,
    #[serde(default)]
    pub volatile_celery_headers: Vec<NamedRule>,
    #[serde(default)]
    pub volatile_db_columns: Vec<DbRule>,
    #[serde(default = "default_timestamp_slack")]
    pub timestamp_slack_secs: i64,
    #[serde(default = "default_db_schemas")]
    pub db_schemas: Vec<String>,
    #[serde(default)]
    pub db_exclude_tables: Vec<String>,
}

impl NormalizeRules {
    pub fn load(path: &Path) -> Result<Self> {
        let rules: Self = read_yaml(path)?;
        rules.validate()?;
        Ok(rules)
    }

    fn validate(&self) -> Result<()> {
        if self.timestamp_slack_secs < 0 {
            bail!("normalize: `timestamp_slack_secs` must be >= 0");
        }
        for rule in self
            .volatile_cookies
            .iter()
            .chain(&self.volatile_json_keys)
            .chain(&self.volatile_celery_headers)
        {
            if rule.name.is_empty() {
                bail!("normalize: rule with an empty name");
            }
            if rule.why.trim().is_empty() {
                bail!("normalize: rule {:?} has no justification", rule.name);
            }
        }
        for rule in &self.volatile_db_columns {
            if rule.table.is_empty() || rule.column.is_empty() {
                bail!("normalize: database rule with an empty table or column");
            }
            if rule.why.trim().is_empty() {
                bail!(
                    "normalize: rule {}.{} has no justification",
                    rule.table,
                    rule.column
                );
            }
        }
        for schema in &self.db_schemas {
            if schema.is_empty() {
                bail!("normalize: empty schema name");
            }
        }
        Ok(())
    }

    pub fn ignore_header(&self, name: &str) -> bool {
        self.ignore_headers
            .iter()
            .any(|ignored| ignored.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scenario_with_two_bodies_is_rejected() {
        let yaml = r#"
name: t
steps:
  - name: s
    path: /x
    json: {a: 1}
    body: "hello"
"#;
        let scenario: Scenario = serde_yaml_ng::from_str(yaml).expect("parse");
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("at most one"));
    }

    #[test]
    fn duplicate_step_names_are_rejected() {
        let yaml = r#"
name: t
steps:
  - name: s
    path: /x
  - name: s
    path: /y
"#;
        let scenario: Scenario = serde_yaml_ng::from_str(yaml).expect("parse");
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("duplicate step"));
    }

    #[test]
    fn capture_source_is_validated() {
        let yaml = r#"
name: t
steps:
  - name: s
    path: /x
    capture:
      - name: v
        source: json
"#;
        let scenario: Scenario = serde_yaml_ng::from_str(yaml).expect("parse");
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs `pointer`"));
    }

    #[test]
    fn db_rule_requires_a_justification() {
        let yaml = r#"
volatile_db_columns:
  - table: sessions
    column: session_key
    why: ""
"#;
        let rules: NormalizeRules = serde_yaml_ng::from_str(yaml).expect("parse");
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("no justification"));
    }
}
