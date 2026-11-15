// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Typed views of the three YAML files the harness reads: `targets.yaml`,
//! `normalize.yaml` and scenario files.
//!
//! Everything is validated up front so a typo in a scenario fails before any
//! container, database or target is touched. The normalisation rules are
//! validated against the shape catalogue: a rule that placeholders a value must
//! name a shape, and every shape must be a valid regular expression with a
//! justification.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

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

fn default_secret_key_var() -> String {
    "SECRET_KEY".to_string()
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
    /// `django`, `rust`, ... A label: the harness never branches on it.
    pub kind: String,
    /// Inclusive port range the harness may allocate from.
    #[serde(default = "default_port_range")]
    pub port_range: Vec<u16>,
    /// Command template; the first element is relative to the repository root.
    pub command: Vec<String>,
    /// Environment template. Placeholders: `{port}`, `{db}`, `{redis_db}`,
    /// `{vhost}`, `{repo}`. Overrides variables produced by `env_from`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Shell script sourced in a clean environment before the target starts;
    /// every variable it exports is passed to the target. This is how the
    /// `rust` target gets exactly the environment `dev/django-env.sh` gives
    /// Django. The script sees `PLANE_DB`, `PLANE_REDIS_DB` and
    /// `PLANE_MQ_VHOST` for this target in its own environment.
    #[serde(default)]
    pub env_from: Option<String>,
    /// File holding the target's Django `SECRET_KEY` (dotenv syntax). The
    /// harness reads it to verify signed session payloads and never prints or
    /// stores the value.
    #[serde(default)]
    pub secret_key_file: Option<String>,
    /// Variable name to read from `secret_key_file`.
    #[serde(default = "default_secret_key_var")]
    pub secret_key_var: String,
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
        if self.secret_key_file.is_some() && self.secret_key_var.trim().is_empty() {
            bail!("target {name}: `secret_key_var` must not be empty");
        }
        Ok(())
    }

    /// Read `secret_key_var` out of `secret_key_file`, when configured.
    ///
    /// Errors name the file and variable only: the value must never reach a
    /// log, a report or an error message. `FILE` is a template: the caller
    /// renders `{repo}` first.
    pub fn secret_key(&self, repo_root: &Path) -> Result<Option<Vec<u8>>> {
        let Some(file) = &self.secret_key_file else {
            return Ok(None);
        };
        let path = render_repo_template(file, repo_root);
        let value = read_env_value(&path, &self.secret_key_var)?;
        if value.is_empty() {
            bail!(
                "{} is empty in {} (needed to verify session payloads)",
                self.secret_key_var,
                path.display()
            );
        }
        Ok(Some(value.into_bytes()))
    }
}

fn render_repo_template(template: &str, repo_root: &Path) -> std::path::PathBuf {
    std::path::PathBuf::from(template.replace("{repo}", &repo_root.display().to_string()))
}

/// Read one variable out of a dotenv-style file. No value is ever included in
/// an error message.
pub fn read_env_value(path: &Path, key: &str) -> Result<String> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} for {key}", path.display()))?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim();
        let value = if let Some(inner) = value.strip_prefix('"') {
            inner.strip_suffix('"').unwrap_or(inner)
        } else if let Some(inner) = value.strip_prefix('\'') {
            inner.strip_suffix('\'').unwrap_or(inner)
        } else {
            value
        };
        return Ok(value.to_string());
    }
    bail!("{} is not set in {}", key, path.display())
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(default = "default_template_db")]
    pub template_db: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, String>,
    /// Scenario variables that hold plaintext passwords. When a `users.password`
    /// column changes, the harness re-derives the PBKDF2 hash and fails unless
    /// one of these plaintexts verifies. The variable is resolved per target,
    /// so `--b-var password=...` also changes what target B must verify against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub password_vars: Vec<String>,
    /// Redis database indexes used when the harness starts the targets:
    /// [A, B]. Defaults to [1, 2].
    #[serde(default = "default_redis_db")]
    pub redis_db: Vec<i64>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
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
    #[serde(default = "default_jar")]
    pub jar: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capture: Vec<Capture>,
    /// Sanity check applied to both targets. A mismatch is reported like any
    /// other difference, so an unexpected status cannot pass unnoticed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Query {
    Map(BTreeMap<String, String>),
    Raw(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub name: String,
    pub source: CaptureSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("scenario: `name` must not be empty");
        }
        if self.redis_db.len() != 2 {
            bail!("scenario: `redis_db` must contain exactly two indexes");
        }
        if self.steps.is_empty() {
            bail!("scenario {}: no steps", self.name);
        }
        for variable in &self.password_vars {
            if !self.vars.contains_key(variable) {
                bail!(
                    "scenario {}: password_vars entry {variable:?} is not a scenario variable",
                    self.name
                );
            }
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

/// A named value shape. Rules reference a shape by name; the pattern is
/// checked *before* a value is placeholdered, on both targets, so a value of
/// the wrong length or alphabet is a difference rather than a normalised blob.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeRule {
    pub pattern: String,
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct Shape {
    pub name: String,
    pub regex: regex::Regex,
    pub why: String,
}

impl Shape {
    pub fn is_match(&self, value: &str) -> bool {
        self.regex.is_match(value)
    }
}

/// How a rule rewrites a value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleMode {
    /// Replace the value with a typed placeholder from this target's map:
    /// the same raw value always becomes the same `<placeholder:N>`.
    #[default]
    Placeholder,
    /// Like `placeholder`, but an empty value or `null` is left alone.
    PlaceholderIfNonEmpty,
    /// A timestamp relative to the step clock: replace with `<offset+Ns>`
    /// (rounded to 10 s) so `now + a fixed age` is compared by its age.
    RelativeTime,
    /// A Unix timestamp in seconds close to the step clock: replace with
    /// `<epoch>`.
    UnixTime,
    /// A Django signed session payload: verify the signature with the target's
    /// `SECRET_KEY`, decode it, and replace the value with the decoded
    /// dictionary so both targets compare structurally.
    SessionPayload,
    /// A Django password hash: verify it against the declared plaintexts, keep
    /// `algorithm$iterations$` exactly and placeholder the salt and digest.
    Password,
    /// Drop the value entirely. Only used for Celery transport headers, which
    /// are not part of the task's observable behaviour.
    Drop,
}

/// A check that needs something outside the value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCheck {
    /// The value is a masked CSRF token; it must unmask to the `csrftoken`
    /// cookie secret set by the same response.
    CsrfMasked,
}

/// A named normalisation rule. `why` is mandatory and is copied into the run
/// report, so every exemption carries its justification with it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedRule {
    pub name: String,
    pub why: String,
    #[serde(default)]
    pub mode: RuleMode,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub check: Option<RuleCheck>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbRule {
    pub table: String,
    pub column: String,
    pub why: String,
    #[serde(default)]
    pub mode: RuleMode,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub check: Option<RuleCheck>,
}

/// The parts `NamedRule` and `DbRule` share, after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSpec {
    pub mode: RuleMode,
    pub placeholder: Option<String>,
    pub shape: Option<String>,
    pub check: Option<RuleCheck>,
    pub why: String,
}

impl NamedRule {
    pub fn spec(&self) -> RuleSpec {
        RuleSpec {
            mode: self.mode,
            placeholder: self.placeholder.clone(),
            shape: self.shape.clone(),
            check: self.check,
            why: self.why.clone(),
        }
    }
}

impl DbRule {
    pub fn spec(&self) -> RuleSpec {
        RuleSpec {
            mode: self.mode,
            placeholder: self.placeholder.clone(),
            shape: self.shape.clone(),
            check: self.check,
            why: self.why.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizeRules {
    #[serde(default = "default_ignore_headers")]
    pub ignore_headers: Vec<String>,
    /// Shapes referenced by the rules below.
    #[serde(default)]
    pub shapes: BTreeMap<String, ShapeRule>,
    #[serde(default)]
    pub volatile_cookies: Vec<NamedRule>,
    #[serde(default)]
    pub volatile_json_keys: Vec<NamedRule>,
    #[serde(default)]
    pub volatile_headers: Vec<NamedRule>,
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
        for (name, shape) in &self.shapes {
            if name.trim().is_empty() {
                bail!("normalize: shape with an empty name");
            }
            if shape.why.trim().is_empty() {
                bail!("normalize: shape {name} has no justification");
            }
            regex::Regex::new(&shape.pattern).map_err(|error| {
                anyhow::anyhow!("normalize: shape {name}: invalid regex: {error}")
            })?;
        }
        for (what, rule) in self
            .volatile_cookies
            .iter()
            .map(|rule| ("cookie", (rule.name.as_str(), rule.spec())))
            .chain(
                self.volatile_json_keys
                    .iter()
                    .map(|rule| ("json key", (rule.name.as_str(), rule.spec()))),
            )
            .chain(
                self.volatile_headers
                    .iter()
                    .map(|rule| ("header", (rule.name.as_str(), rule.spec()))),
            )
            .chain(
                self.volatile_celery_headers
                    .iter()
                    .map(|rule| ("celery header", (rule.name.as_str(), rule.spec()))),
            )
        {
            if rule.0.trim().is_empty() {
                bail!("normalize: {what} rule with an empty name");
            }
            validate_spec(what, rule.0, &rule.1, &self.shapes)?;
        }
        for rule in &self.volatile_db_columns {
            if rule.table.is_empty() || rule.column.is_empty() {
                bail!("normalize: database rule with an empty table or column");
            }
            let name = format!("{}.{}", rule.table, rule.column);
            validate_spec("database column", &name, &rule.spec(), &self.shapes)?;
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

    /// True when the rules can only be applied with a target `SECRET_KEY`.
    pub fn requires_secret_key(&self) -> bool {
        self.all_specs()
            .any(|spec| spec.mode == RuleMode::SessionPayload)
    }

    fn all_specs(&self) -> impl Iterator<Item = RuleSpec> + '_ {
        self.volatile_cookies
            .iter()
            .map(NamedRule::spec)
            .chain(self.volatile_json_keys.iter().map(NamedRule::spec))
            .chain(self.volatile_headers.iter().map(NamedRule::spec))
            .chain(self.volatile_celery_headers.iter().map(NamedRule::spec))
            .chain(self.volatile_db_columns.iter().map(DbRule::spec))
    }

    /// Compile the shape catalogue. Called once per run; the normaliser holds
    /// the compiled regexes.
    pub fn compile_shapes(&self) -> BTreeMap<String, Shape> {
        self.shapes
            .iter()
            .filter_map(|(name, rule)| {
                regex::Regex::new(&rule.pattern).ok().map(|regex| {
                    (
                        name.clone(),
                        Shape {
                            name: name.clone(),
                            regex,
                            why: rule.why.clone(),
                        },
                    )
                })
            })
            .collect()
    }
}

fn validate_spec(
    what: &str,
    name: &str,
    spec: &RuleSpec,
    shapes: &BTreeMap<String, ShapeRule>,
) -> Result<()> {
    if spec.why.trim().is_empty() {
        bail!("normalize: {what} rule {name:?} has no justification");
    }
    match spec.mode {
        RuleMode::Placeholder | RuleMode::PlaceholderIfNonEmpty => {
            let Some(placeholder) = &spec.placeholder else {
                bail!("normalize: {what} rule {name:?} needs `placeholder`");
            };
            if placeholder.is_empty()
                || !placeholder
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_lowercase())
                || !placeholder
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                bail!(
                    "normalize: {what} rule {name:?}: `placeholder` must be lower-case letters, digits and `-`"
                );
            }
        }
        RuleMode::Password | RuleMode::SessionPayload => {
            if spec.placeholder.is_some() {
                bail!("normalize: {what} rule {name:?}: `placeholder` does not apply to this mode");
            }
        }
        RuleMode::RelativeTime | RuleMode::UnixTime => {
            if spec.placeholder.is_some() {
                bail!("normalize: {what} rule {name:?}: `placeholder` does not apply to this mode");
            }
        }
        RuleMode::Drop => {
            if what != "celery header" {
                bail!(
                    "normalize: {what} rule {name:?}: `mode: drop` only applies to celery headers"
                );
            }
            if spec.placeholder.is_some() || spec.shape.is_some() {
                bail!(
                    "normalize: {what} rule {name:?}: `mode: drop` takes no placeholder or shape"
                );
            }
        }
    }
    if let Some(shape) = &spec.shape
        && !shapes.contains_key(shape)
    {
        bail!("normalize: {what} rule {name:?} references unknown shape {shape:?}");
    }
    let needs_shape = matches!(
        spec.mode,
        RuleMode::Placeholder
            | RuleMode::PlaceholderIfNonEmpty
            | RuleMode::Password
            | RuleMode::SessionPayload
    );
    if needs_shape && spec.shape.is_none() {
        bail!("normalize: {what} rule {name:?} needs `shape`");
    }
    if let Some(check) = spec.check {
        match check {
            RuleCheck::CsrfMasked => {
                if !matches!(
                    spec.mode,
                    RuleMode::Placeholder | RuleMode::PlaceholderIfNonEmpty
                ) {
                    bail!(
                        "normalize: {what} rule {name:?}: `check: csrf_masked` needs a placeholder mode"
                    );
                }
                if what == "database column" {
                    bail!(
                        "normalize: {what} rule {name:?}: `check: csrf_masked` needs a response part"
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> NormalizeRules {
        serde_yaml_ng::from_str(text).expect("parse")
    }

    fn parse_scenario(text: &str) -> Scenario {
        serde_yaml_ng::from_str(text).expect("parse")
    }

    #[test]
    fn a_scenario_with_two_bodies_is_rejected() {
        let scenario = parse_scenario(
            r#"
name: t
steps:
  - name: s
    path: /x
    json: {a: 1}
    body: "hello"
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("at most one"));
    }

    #[test]
    fn duplicate_step_names_are_rejected() {
        let scenario = parse_scenario(
            r#"
name: t
steps:
  - name: s
    path: /x
  - name: s
    path: /y
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("duplicate step"));
    }

    #[test]
    fn capture_source_is_validated() {
        let scenario = parse_scenario(
            r#"
name: t
steps:
  - name: s
    path: /x
    capture:
      - name: v
        source: json
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs `pointer`"));
    }

    #[test]
    fn password_vars_must_name_scenario_variables() {
        let scenario = parse_scenario(
            r#"
name: t
vars:
  password: secret
password_vars: [password, nope]
steps:
  - name: s
    path: /x
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("\"nope\""));
    }

    #[test]
    fn db_rule_requires_a_justification() {
        let rules = parse(
            r#"
volatile_db_columns:
  - table: sessions
    column: session_key
    why: ""
"#,
        );
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("no justification"));
    }

    #[test]
    fn a_placeholder_rule_needs_a_placeholder_and_a_shape() {
        let rules = parse(
            r#"
volatile_cookies:
  - name: session-id
    why: "random key"
"#,
        );
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs `placeholder`"));

        let rules = parse(
            r#"
volatile_cookies:
  - name: session-id
    why: "random key"
    placeholder: session
"#,
        );
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs `shape`"));
    }

    #[test]
    fn a_rule_cannot_reference_an_unknown_shape() {
        let rules = parse(
            r#"
shapes:
  session_key:
    pattern: '^[a-z0-9]{128}$'
    why: "random key"
volatile_cookies:
  - name: session-id
    why: "random key"
    placeholder: session
    shape: session_key_typo
"#,
        );
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("unknown shape"));
    }

    #[test]
    fn an_invalid_shape_regex_is_rejected() {
        let rules = parse(
            r#"
shapes:
  broken:
    pattern: '(unclosed'
    why: "oops"
"#,
        );
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("invalid regex"));
    }

    #[test]
    fn session_payload_rules_require_a_secret_key() {
        let rules = parse(
            r#"
shapes:
  signed_session:
    pattern: '^[^:]+:[^:]+:[^:]+$'
    why: "signed payload"
volatile_db_columns:
  - table: sessions
    column: session_data
    mode: session_payload
    shape: signed_session
    why: "signed payload"
"#,
        );
        rules.validate().expect("valid");
        assert!(rules.requires_secret_key());
    }

    #[test]
    fn csrf_checks_are_only_allowed_on_response_rules() {
        let rules = parse(
            r#"
shapes:
  csrf_token:
    pattern: '^[A-Za-z0-9]{64}$'
    why: "masked token"
volatile_cookies:
  - name: csrftoken
    why: "secret"
    placeholder: csrf-token
    shape: csrf_token
    check: csrf_masked
"#,
        );
        rules.validate().expect("cookie rules may carry the check");

        let rules = parse(
            r#"
shapes:
  csrf_token:
    pattern: '^[A-Za-z0-9]{64}$'
    why: "masked token"
volatile_db_columns:
  - table: x
    column: y
    why: "secret"
    placeholder: csrf-token
    shape: csrf_token
    check: csrf_masked
"#,
        );
        let error = rules.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs a response part"));
    }

    #[test]
    fn env_values_are_read_without_echoing_them() {
        let dir = std::env::temp_dir().join(format!("difftest-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(".env.local");
        std::fs::write(&path, "# comment\nSECRET_KEY=abc123\nOTHER=x\n").expect("write");
        assert_eq!(read_env_value(&path, "SECRET_KEY").expect("read"), "abc123");
        let error = read_env_value(&path, "MISSING").expect_err("must fail");
        assert!(!error.to_string().contains("abc123"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
