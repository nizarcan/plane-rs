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

/// The reserved scenario variable naming a SQL fixture (see
/// [`Scenario::fixture_sql`]).
pub const FIXTURE_SQL_VAR: &str = "fixture_sql";
/// The reserved scenario variable naming a Redis fixture (see
/// [`Scenario::fixture_redis`]).
pub const FIXTURE_REDIS_VAR: &str = "fixture_redis";

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

fn default_true() -> bool {
    true
}

fn default_origin() -> OriginHeader {
    OriginHeader::Bool(true)
}

/// `origin:` on a step: `true` derives it from `Host`, `false` omits it, a
/// string is the literal value.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum OriginHeader {
    Bool(bool),
    Text(String),
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

/// Environment variables a scenario may not set through `target_env`.
///
/// Each one selects a resource the harness gives to exactly one target: the
/// database clone, the Redis logical database, the broker vhost and the bind
/// address. A scenario value that overrode one of them would make both targets
/// share a resource, so the differences a run reports would no longer be
/// attributable to one backend.
pub const RESERVED_TARGET_ENV: &[&str] = &[
    "DATABASE_URL",
    "REDIS_URL",
    "PLANE_DB",
    "PLANE_REDIS_DB",
    "PLANE_MQ_VHOST",
    "PLANE_RS_BIND",
    // The broker is selected by these two as well; `django-env.sh` builds
    // CELERY_BROKER_URL from RABBITMQ_* and refuses AMQP_URL.
    "RABBITMQ_VHOST",
    "AMQP_URL",
    "CELERY_BROKER_URL",
];

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
    /// A copy of this target with `extra` merged into `env`.
    ///
    /// `extra` wins over a key already present, and both are applied after
    /// `env_from` when the target starts, which is exactly where a scenario's
    /// `target_env` belongs: the value must hold for both targets and may
    /// override a default from `targets.yaml`.
    pub fn with_env(&self, extra: &BTreeMap<String, String>) -> Self {
        let mut spec = self.clone();
        for (key, value) in extra {
            spec.env.insert(key.clone(), value.clone());
        }
        spec
    }

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
    /// Extra environment for both managed targets, merged into each target's
    /// `env:` map (which is applied after `env_from`, so it also wins over a
    /// value in `targets.yaml`). Use it for a setting the scenario itself
    /// depends on and that must be identical on both sides, for example
    /// `FILE_SIZE_LIMIT: "4096"`. Variables that select a per-target resource
    /// (database, Redis index, broker vhost, bind address) are rejected; see
    /// [`RESERVED_TARGET_ENV`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub target_env: BTreeMap<String, String>,
    /// Compare drained Celery messages on a `policy-probe` pass-through step.
    ///
    /// A pass-through step normally ignores every side effect, because target A
    /// ran a real handler and B ran the probe stub. A middleware side effect —
    /// `APITokenLogMiddleware`'s `process_logs` message — is produced below the
    /// handler on **both** sides, so it is comparable. Exactly two `log_data`
    /// fields copy the response the pass-through rule already ignores
    /// (`response_code`, `response_body`); they are masked on both sides and
    /// everything else is compared byte for byte. Only set it for steps whose
    /// real handler enqueues nothing of its own.
    #[serde(default)]
    pub probe_compare_celery: bool,
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
    /// `multipart/form-data` with text fields and file parts. The boundary is a
    /// constant, so both targets receive byte-identical request bodies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multipart: Option<Multipart>,
    #[serde(default = "default_jar")]
    pub jar: String,
    /// Attach `X-CSRFToken` from this jar's `csrftoken` cookie on an unsafe
    /// method (default `true`). `false` omits it, which is how a scenario
    /// asserts the CSRF rejection.
    #[serde(default = "default_true")]
    pub csrf: bool,
    /// Send this value as `X-Api-Key` (API-token auth, for `/api/v1/`). No
    /// session cookie is needed; the jar still supplies one if it has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// `Origin` on an unsafe, cookie-authenticated request. Plane replaces
    /// DRF's token CSRF with an origin check (`BaseSessionAuthentication`):
    /// a missing header is accepted (non-browser clients), a header must name
    /// the request's own host. `true` (default) sends the step's `Host` as the
    /// origin, `false` omits it, and a string is sent verbatim — the rejection
    /// case.
    #[serde(default = "default_origin")]
    pub origin: OriginHeader,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capture: Vec<Capture>,
    /// Sanity check applied to both targets. A mismatch is reported like any
    /// other difference, so an unexpected status cannot pass unnoticed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_status: Option<u16>,
}

/// A `multipart/form-data` body: text fields first, then file parts, in the
/// order they appear in the YAML (the serialised body is compared only via the
/// response, but a stable order keeps two runs identical).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Multipart {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FilePart>,
}

/// One file part of a multipart body. `content` is inline text; `path` reads a
/// file relative to the harness's working directory (`apps/server`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FilePart {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
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
    /// `db` captures: the table whose **inserted** rows this step produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// `db` captures: the inserted row's column, e.g. `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
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
    /// A column of the step's **inserted** database rows: the `id` of the row a
    /// `POST` created, without parsing the response body. The step must insert
    /// exactly one row in `table`; a write that inserts several (or none) is an
    /// error, not a guess.
    Db,
}

impl Scenario {
    /// A SQL fixture applied to both databases before the baseline snapshot.
    ///
    /// This is a **reserved scenario variable**, not a request variable:
    /// `vars: { fixture_sql: "{repo}/scenarios/policy-fixture.sql" }`. The
    /// policy probe needs users, sessions and one object per URL kwarg, and
    /// `/auth/` and `/api/` are not ported, so it cannot create them over
    /// HTTP. `tools/policy/gen_fixture.py` writes the deterministic SQL (fixed
    /// UUIDs, `ON CONFLICT DO NOTHING` so replaying against attached databases
    /// is idempotent); it is applied to both clones and is part of the
    /// baseline, never a step diff.
    ///
    /// It is a variable rather than a new `Scenario` field so that recording
    /// and benchmarking scenarios do not have to grow a field they never use.
    pub fn fixture_sql(&self) -> Option<std::path::PathBuf> {
        self.vars.get(FIXTURE_SQL_VAR).map(|path| {
            std::path::PathBuf::from(
                path.replace("{repo}", &crate::util::repo_root().display().to_string()),
            )
        })
    }

    /// A Redis fixture applied to **both targets' own** Redis databases
    /// before the baseline Redis snapshot.
    ///
    /// `vars: { fixture_redis: "{repo}/scenarios/write/users-me-email-fixture.redis.json" }`.
    /// The file is a JSON object `{"entries": [{"key": …, "value": …,
    /// "timeout": …}]}`; `key` is the logical django-redis key (the harness
    /// prepends `:1:`), `value` follows `django_redis`'s encoder (an integer
    /// is its ASCII digits, everything else a protocol-4 pickle) and `timeout`
    /// is seconds (`null`/absent means no expiry). This is how a scenario
    /// starts with a value only Python's side effects would normally create —
    /// for example the 6-digit e-mail-change code
    /// (`plane/app/views/user/base.py:170-175`), whose value and task argument
    /// are random. `crate::fixture_redis` owns the format and its tests.
    pub fn fixture_redis(&self) -> Option<std::path::PathBuf> {
        self.vars.get(FIXTURE_REDIS_VAR).map(|path| {
            std::path::PathBuf::from(
                path.replace("{repo}", &crate::util::repo_root().display().to_string()),
            )
        })
    }

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
        for key in self.target_env.keys() {
            if key.trim().is_empty() {
                bail!(
                    "scenario {}: `target_env` has an empty variable name",
                    self.name
                );
            }
            if RESERVED_TARGET_ENV.contains(&key.as_str()) {
                bail!(
                    "scenario {}: `target_env` may not set {key}: the harness gives each \
                     target its own database, Redis index, broker vhost and bind address",
                    self.name
                );
            }
        }
        if let Some(path) = self.fixture_sql()
            && !path.is_file()
        {
            bail!("scenario {}: FIXTURE_SQL {path:?} is not a file", self.name);
        }
        if let Some(path) = self.fixture_redis() {
            if !path.is_file() {
                bail!(
                    "scenario {}: FIXTURE_REDIS {path:?} is not a file",
                    self.name
                );
            }
            crate::fixture_redis::load(&path)
                .map_err(|error| anyhow::anyhow!("scenario {}: {error}", self.name))?;
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
                + usize::from(step.body.is_some())
                + usize::from(step.multipart.is_some());
            if bodies > 1 {
                bail!("{where_}: at most one of `json`, `form`, `body`, `multipart` may be set");
            }
            if let OriginHeader::Text(text) = &step.origin
                && text.trim().is_empty()
            {
                bail!("{where_}: `origin` must not be an empty string");
            }
            if let Some(multipart) = &step.multipart {
                for (index, file) in multipart.files.iter().enumerate() {
                    if file.field.trim().is_empty() {
                        bail!("{where_}: multipart file {index} has an empty `field`");
                    }
                    if file.content.is_some() == file.path.is_some() {
                        bail!(
                            "{where_}: multipart file {:?} needs exactly one of `content` or `path`",
                            file.field
                        );
                    }
                }
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
            CaptureSource::Db => {
                if self.table.is_none() || self.column.is_none() {
                    bail!("{where_}: capture {} needs `table` and `column`", self.name);
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
    /// Like `placeholder`, but a value that already exists in the baseline
    /// snapshot stays literal. DECISIONS.md D16: a value the fixture seeded is
    /// identical on both sides and must be compared, not hidden; only a value
    /// generated during the run (a fresh `uuid4().hex`, say) needs a
    /// placeholder so the two targets can agree on it.
    PlaceholderIfNew,
    /// Like `placeholder`, but only when the value matches the rule's `shape`.
    /// A value that does not look generated stays literal and is compared —
    /// e.g. an API-token `label` supplied in the create body, versus the
    /// `uuid4().hex` default the model invents when it is missing. A shape
    /// failure is *not* a violation here; it means "this was not generated".
    PlaceholderIfShape,
    /// A timestamp relative to the step clock: replace with `<offset+Ns>`
    /// (rounded to 10 s) so `now + a fixed age` is compared by its age.
    RelativeTime,
    /// A Unix timestamp in seconds close to the step clock: replace with
    /// `<epoch>`.
    UnixTime,
    /// An HTTP date (RFC 5322) compared as an offset from the step clock,
    /// rounded to 10 s — the same rule a cookie's `Expires` gets. Used for a
    /// page-cache response's `Expires`, which is `http_date(now + timeout)`
    /// (`django/utils/cache.py:patch_response_headers`).
    HttpDate,
    /// A SigV4 signing date (`YYYYMMDDTHHMMSSZ`), the `x-amz-date` field of a
    /// presigned POST. The two targets sign at their own instants, so the value
    /// legitimately differs by up to two seconds when a step straddles a second
    /// boundary — the same fact DECISIONS.md D17 records for a presigned GET
    /// URL. The value is parsed and checked against the step window (so a
    /// wrong clock or a replayed body still fails) and replaced by one
    /// constant, because nothing can be compared byte-wise across two signing
    /// instants. This replaces a per-value placeholder, which failed a run
    /// whenever exactly one of the two targets crossed a second boundary.
    Sigv4Date,
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
    /// A presigned S3 URL (DECISIONS.md D17): everything except `X-Amz-Date`
    /// and `X-Amz-Signature` must be byte-identical, and each side's signature
    /// is recomputed with that target's own credentials and `X-Amz-Date`.
    ///
    /// The value is verified and rewritten to a canonical form whose date is
    /// `<presigned-date:<epoch>>` and whose signature is
    /// `<presigned-signature>`; the comparator then accepts two dates at most
    /// two seconds apart and everything else exactly.
    PresignedUrl,
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
    /// A glob over the step's `METHOD path?query`. The rule only applies to a
    /// matching step; every other step is normalised by the default rules, so a
    /// mode like `presigned_url` cannot relax a header on an unrelated route.
    #[serde(default)]
    pub request: Option<String>,
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
    /// A glob over the step's `METHOD path?query`, like `NamedRule`'s. A
    /// request-scoped DB rule leaves every other step's column alone.
    #[serde(default)]
    pub request: Option<String>,
}

/// A JSON rule scoped by **document path** instead of key name.
///
/// The name-keyed tables (`volatile_json_keys`, `volatile_db_columns`) hold at
/// most one rule per name/column, so a `token` rule for a new API token
/// (`plane_api_…`) would replace the `users.token` rule (`64` hex). A path
/// rule is looked up at the exact JSON path first, so `$.token` on
/// `POST /api/users/api-tokens/` can have its own shape while every other
/// `token` keeps the name-keyed one.
///
/// `path` uses the same grammar as [`UnorderedPathRule::path`]: `$` is the
/// root, `*` one component and `**` any number. A rule without `request`
/// applies to every response body, so scope it unless the path is unique.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonPathRule {
    pub path: String,
    pub why: String,
    #[serde(default)]
    pub mode: RuleMode,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub check: Option<RuleCheck>,
    /// A glob over the step's `METHOD path?query`, like [`NamedRule`]'s.
    #[serde(default)]
    pub request: Option<String>,
}

impl JsonPathRule {
    pub fn spec(&self) -> RuleSpec {
        RuleSpec {
            mode: self.mode,
            placeholder: self.placeholder.clone(),
            shape: self.shape.clone(),
            check: self.check,
            request: self.request.clone(),
            why: self.why.clone(),
        }
    }
}

/// A regex over every **string** the normaliser rewrites (JSON values, Celery
/// reprs, Redis values, response text). Used for a random value that has no
/// JSON key of its own — the six-digit e-mail-change code
/// (`plane/app/views/user/base.py:170`) appears inside `argsrepr` and inside
/// the pickled `{"token": …}` cache value.
///
/// `request` is mandatory for the same reason as [`JsonPathRule`]'s: a bare
/// six-digit pattern would rewrite unrelated numbers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatternRule {
    pub pattern: String,
    pub placeholder: String,
    pub why: String,
    /// A glob over the step's `METHOD path?query`.
    pub request: String,
}

/// The parts `NamedRule` and `DbRule` share, after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSpec {
    pub mode: RuleMode,
    pub placeholder: Option<String>,
    pub shape: Option<String>,
    pub check: Option<RuleCheck>,
    pub request: Option<String>,
    pub why: String,
}

impl NamedRule {
    pub fn spec(&self) -> RuleSpec {
        RuleSpec {
            mode: self.mode,
            placeholder: self.placeholder.clone(),
            shape: self.shape.clone(),
            check: self.check,
            request: self.request.clone(),
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
            request: self.request.clone(),
            why: self.why.clone(),
        }
    }
}

/// How an `unordered_json_paths` rule rewrites the document at its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnorderedMode {
    /// Sort the array's elements (a multiset: a missing, extra or duplicated
    /// element still differs, only the order is ignored).
    Array,
    /// Compare the object's **key order** as unordered; the key set and every
    /// value are still compared exactly.
    ObjectKeys,
    /// Replace the scalar with the path component that matched the pattern's
    /// first `*` — for the raw group column of a row that Django copies into
    /// every group an issue belongs to, whose value is whichever of the
    /// issue's rows the database returned first.
    ParentKey,
}

/// A path-scoped unordered comparison (DECISIONS.md D16).
///
/// `path` is a glob over the JSON document: `$` is the root, `.key` and
/// `[index]` are path components, `*` matches exactly one component (object
/// key or array index) and `**` matches any number of them. `request` is an
/// optional glob over the step's `METHOD path?query`, so a rule can be limited
/// to the endpoints whose response Django builds from a Python `set`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnorderedPathRule {
    pub path: String,
    pub mode: UnorderedMode,
    #[serde(default)]
    pub request: Option<String>,
    pub why: String,
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
    /// Path-scoped JSON rules (see [`JsonPathRule`]).
    #[serde(default)]
    pub volatile_json_paths: Vec<JsonPathRule>,
    /// Request-scoped regexes over strings (see [`TextPatternRule`]).
    #[serde(default)]
    pub volatile_text_patterns: Vec<TextPatternRule>,
    #[serde(default = "default_timestamp_slack")]
    pub timestamp_slack_secs: i64,
    #[serde(default = "default_db_schemas")]
    pub db_schemas: Vec<String>,
    #[serde(default)]
    pub db_exclude_tables: Vec<String>,
    /// Path-scoped unordered rules (JSON document paths and an optional
    /// request glob). See [`UnorderedPathRule`].
    #[serde(default)]
    pub unordered_json_paths: Vec<UnorderedPathRule>,
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
        for rule in &self.volatile_json_paths {
            if !rule.path.starts_with('$') {
                bail!(
                    "normalize: volatile_json_paths path {:?} must start with `$`",
                    rule.path
                );
            }
            validate_spec("JSON path", &rule.path, &rule.spec(), &self.shapes)?;
        }
        for rule in &self.volatile_text_patterns {
            if rule.placeholder.trim().is_empty() {
                bail!(
                    "normalize: text-pattern rule {:?} has an empty placeholder",
                    rule.pattern
                );
            }
            if rule.request.trim().is_empty() {
                bail!(
                    "normalize: text-pattern rule {:?} needs a request glob, or it                      would rewrite every response",
                    rule.pattern
                );
            }
            regex::Regex::new(&rule.pattern).map_err(|error| {
                anyhow::anyhow!(
                    "normalize: text-pattern rule {:?}: invalid regex: {error}",
                    rule.pattern
                )
            })?;
            if rule.why.trim().is_empty() {
                bail!(
                    "normalize: text-pattern rule {:?} has no justification",
                    rule.pattern
                );
            }
        }
        for rule in &self.unordered_json_paths {
            if rule.why.trim().is_empty() {
                bail!(
                    "normalize: unordered_json_paths rule {} has no justification",
                    rule.path
                );
            }
            if !rule.path.starts_with('$') {
                bail!(
                    "normalize: unordered_json_paths path {:?} must start with `$`",
                    rule.path
                );
            }
            if let Some(request) = &rule.request
                && request.trim().is_empty()
            {
                bail!(
                    "normalize: unordered_json_paths rule {} has an empty request glob",
                    rule.path
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
            .chain(self.volatile_json_paths.iter().map(JsonPathRule::spec))
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
    if let Some(request) = &spec.request
        && request.trim().is_empty()
    {
        bail!("normalize: {what} rule {name:?} has an empty `request` glob");
    }
    match spec.mode {
        RuleMode::Placeholder
        | RuleMode::PlaceholderIfNonEmpty
        | RuleMode::PlaceholderIfNew
        | RuleMode::PlaceholderIfShape => {
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
        RuleMode::Password
        | RuleMode::SessionPayload
        | RuleMode::PresignedUrl
        | RuleMode::Sigv4Date => {
            if spec.placeholder.is_some() {
                bail!("normalize: {what} rule {name:?}: `placeholder` does not apply to this mode");
            }
        }
        RuleMode::RelativeTime | RuleMode::UnixTime | RuleMode::HttpDate => {
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
    fn db_capture_requires_table_and_column() {
        let scenario = parse_scenario(
            r#"
name: t
steps:
  - name: s
    path: /x
    capture:
      - name: created
        source: db
        table: stickies
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("needs `table` and `column`"));
    }

    #[test]
    fn write_step_defaults_and_overrides() {
        let scenario = parse_scenario(
            r#"
name: t
steps:
  - name: create
    method: POST
    path: /x
  - name: update
    method: PATCH
    path: /y
    csrf: false
    origin: "http://evil.example"
    api_key: "{{api_token}}"
"#,
        );
        scenario.validate().expect("valid");
        assert!(scenario.steps[0].csrf);
        assert!(matches!(scenario.steps[0].origin, OriginHeader::Bool(true)));
        assert!(!scenario.steps[1].csrf);
        assert!(matches!(
            &scenario.steps[1].origin,
            OriginHeader::Text(value) if value == "http://evil.example"
        ));
        assert_eq!(scenario.steps[1].api_key.as_deref(), Some("{{api_token}}"));
    }

    #[test]
    fn multipart_files_need_exactly_one_content_source() {
        let scenario = parse_scenario(
            r#"
name: t
steps:
  - name: upload
    method: POST
    path: /x
    multipart:
      files:
        - field: file
          content: "hello"
          path: fixtures/harness.png
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(
            error
                .to_string()
                .contains("exactly one of `content` or `path`")
        );
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
    fn target_env_is_parsed_and_allowed() {
        let scenario = parse_scenario(
            r#"
name: t
target_env:
  FILE_SIZE_LIMIT: "4096"
steps:
  - name: s
    path: /x
"#,
        );
        scenario.validate().expect("valid");
        assert_eq!(
            scenario
                .target_env
                .get("FILE_SIZE_LIMIT")
                .map(String::as_str),
            Some("4096")
        );
    }

    #[test]
    fn target_env_may_not_select_a_per_target_resource() {
        for key in RESERVED_TARGET_ENV {
            let scenario = parse_scenario(&format!(
                r#"
name: t
target_env:
  {key}: "x"
steps:
  - name: s
    path: /x
"#
            ));
            let error = scenario.validate().expect_err(key);
            let message = error.to_string();
            assert!(message.contains(key), "{message}");
            assert!(
                message.contains("its own database"),
                "the message must say why: {message}"
            );
        }
    }

    #[test]
    fn target_env_rejects_an_empty_name() {
        let scenario = parse_scenario(
            r#"
name: t
target_env:
  "": "x"
steps:
  - name: s
    path: /x
"#,
        );
        let error = scenario.validate().expect_err("must reject");
        assert!(error.to_string().contains("empty variable name"));
    }

    #[test]
    fn target_env_wins_over_the_target_env_map() {
        let file: TargetsFile = serde_yaml_ng::from_str(
            r#"
targets:
  rust:
    kind: rust
    command: ["/bin/true"]
    env:
      FILE_SIZE_LIMIT: "5242880"
      PLANE_RS_BIND: "127.0.0.1:{port}"
    health: /
"#,
        )
        .expect("parse");
        let spec = file.get("rust").expect("target");
        let merged = spec.with_env(&BTreeMap::from([(
            "FILE_SIZE_LIMIT".to_string(),
            "4096".to_string(),
        )]));
        assert_eq!(
            merged.env.get("FILE_SIZE_LIMIT").map(String::as_str),
            Some("4096")
        );
        // Other keys are untouched, and the original target is not modified.
        assert_eq!(
            merged.env.get("PLANE_RS_BIND").map(String::as_str),
            Some("127.0.0.1:{port}")
        );
        assert_eq!(
            spec.env.get("FILE_SIZE_LIMIT").map(String::as_str),
            Some("5242880")
        );
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
