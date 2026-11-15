// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Process configuration, built from the environment exactly as `apps/api` reads it.
//!
//! The specification is `apps/server/ENV.md`; every field below cites the table
//! row it reproduces. Two rules from that document shape the design:
//!
//! * **Unset and empty are different.** `os.environ.get("X")` returns `None`
//!   when unset and `""` when set-but-empty, and `int("")` is a startup crash
//!   while `int("0")` is not. Fields that need the distinction are
//!   `Option<String>`; fields read with a default use [`EnvReader::get_str`]
//!   (`os.environ.get(key, default)`) or [`EnvReader::or`] (`env or default`).
//! * **Django crashes at import, so we refuse to start.** `DEBUG=true`,
//!   `FILE_SIZE_LIMIT=""` and friends abort every Python role when the settings
//!   module is imported. [`Settings::from_env`] returns a [`ConfigError`] naming
//!   the variable and the process exits non-zero (see `bins/plane-api`).
//!
//! One deliberate departure, recorded in `PROGRESS.md`: `REDIS_URL` is required
//! by the roles that use Redis (`api`, `worker`, `beat`) but not by `migrator`.
//! Django fails for every role because `plane/__init__.py` imports Celery, which
//! builds a Redis client at import time; that coupling is accidental (ENV.md §0).

use std::borrow::Cow;
use std::fmt;
use std::net::SocketAddr;

use ipnet::IpNet;

use crate::py;

// ---------------------------------------------------------------------------
// Environment access
// ---------------------------------------------------------------------------

/// One read of the process environment.
pub trait EnvSource {
    /// `os.environ.get(key)`: `None` when unset, `Some("")` when set-but-empty.
    fn get(&self, key: &str) -> Option<Cow<'_, str>>;
}

/// The real process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, key: &str) -> Option<Cow<'_, str>> {
        // Non-UTF-8 values are treated as unset: Python's `os.environ` decodes
        // with surrogateescape, so such a value can never equal a valid setting
        // for any call site the port reproduces.
        std::env::var(key).ok().map(Cow::Owned)
    }
}

/// An in-memory environment for tests.
#[derive(Debug, Clone, Default)]
pub struct EnvMap {
    values: std::collections::BTreeMap<String, String>,
}

impl EnvMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a variable (empty value allowed).
    pub fn set(mut self, key: &str, value: &str) -> Self {
        self.values.insert(key.to_string(), value.to_string());
        self
    }
}

impl EnvSource for EnvMap {
    fn get(&self, key: &str) -> Option<Cow<'_, str>> {
        self.values
            .get(key)
            .map(|value| Cow::Borrowed(value.as_str()))
    }
}

/// Which process is loading the settings. Only Redis presence differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Api,
    Worker,
    Beat,
    Migrator,
}

impl Role {
    fn needs_redis(self) -> bool {
        matches!(self, Role::Api | Role::Worker | Role::Beat)
    }
}

// ---------------------------------------------------------------------------
// Errors and warnings
// ---------------------------------------------------------------------------

/// A configuration value Django would reject at import time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub variable: Option<String>,
    pub message: String,
}

impl ConfigError {
    fn new(variable: &str, message: impl Into<String>) -> Self {
        Self {
            variable: Some(variable.to_string()),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.variable {
            Some(variable) => write!(f, "{variable}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

/// A value Django accepts but warns about (an invalid `WEBHOOK_ALLOWED_IPS`
/// entry, an invalid `NUM_PROXIES`, ...). Logged once at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning {
    pub variable: String,
    pub message: String,
}

struct EnvReader<'a, S: EnvSource> {
    source: &'a S,
    warnings: Vec<ConfigWarning>,
}

impl<S: EnvSource> EnvReader<'_, S> {
    fn warning(&mut self, variable: &str, message: impl Into<String>) {
        self.warnings.push(ConfigWarning {
            variable: variable.to_string(),
            message: message.into(),
        });
    }

    /// `os.environ.get(key)`: `None` when unset, `Some("")` when empty.
    fn raw(&self, key: &str) -> Option<String> {
        self.source.get(key).map(|value| value.into_owned())
    }

    /// `os.environ.get(key, default)`: an empty value stays empty.
    fn get_str(&self, key: &str, default: &str) -> String {
        self.raw(key).unwrap_or_else(|| default.to_string())
    }

    /// `os.environ.get(key) or default`: `None` and `""` both yield the default.
    fn or(&self, key: &str, default: &str) -> String {
        py::or_default(self.raw(key).as_deref(), default)
    }

    /// `int(os.environ.get(key, default))`: a Python `ValueError` becomes a
    /// startup error naming the variable.
    fn int(&self, key: &str, default: i64) -> Result<i64, ConfigError> {
        let raw = self.get_str(key, &default.to_string());
        self.parse_int(key, &raw)
    }

    fn parse_int(&self, key: &str, raw: &str) -> Result<i64, ConfigError> {
        py::int(raw).map_err(|error| {
            ConfigError::new(
                key,
                format!(
                    "invalid value {raw:?}: {error} (Django calls int({key}) while importing its settings)"
                ),
            )
        })
    }

    /// `os.environ.get(key, default) == "1"`.
    fn flag(&self, key: &str, default: &str) -> bool {
        self.get_str(key, default) == "1"
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// A signing key. Django falls back to a random key when `SECRET_KEY` is unset
/// or empty (`common.py:33`); the port does the same and reports the source so
/// the binary can warn that sessions will not survive a restart.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey {
    value: String,
    random: bool,
}

impl SecretKey {
    pub fn value(&self) -> &str {
        &self.value
    }

    /// True when the value was generated in this process because the
    /// environment did not provide one.
    pub fn is_random(&self) -> bool {
        self.random
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretKey")
            .field("value", &"<redacted>")
            .field("random", &self.random)
            .finish()
    }
}

/// `corsheaders` settings with Plane's configuration (ENV.md §2.3,
/// HTTP_CONTRACT §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorsSettings {
    /// `CORS_ALLOWED_ORIGINS`; empty means "no allow-list".
    pub allowed_origins: Vec<String>,
    /// `CORS_ALLOW_ALL_ORIGINS`: true when no origin list is configured.
    pub allow_all_origins: bool,
    /// `CORS_ALLOW_CREDENTIALS = True`, unconditionally.
    pub allow_credentials: bool,
    /// `django_cors_headers.default_headers + ["X-API-Key"]`, in order.
    pub allow_headers: Vec<&'static str>,
    /// `CORS_ALLOW_METHODS` (the package default).
    pub allow_methods: Vec<&'static str>,
    /// `CORS_PREFLIGHT_MAX_AGE`.
    pub preflight_max_age: u64,
    /// `CORS_EXPOSE_HEADERS` (empty in Plane, so the header is not sent).
    pub expose_headers: Vec<String>,
    /// `CORS_ALLOW_PRIVATE_NETWORK` (false).
    pub allow_private_network: bool,
    /// `CORS_URLS_REGEX` matches every path (`^.*$`).
    pub urls_regex_all: bool,
}

impl CorsSettings {
    pub const DEFAULT_ALLOW_HEADERS: &'static [&'static str] = &[
        "accept",
        "authorization",
        "content-type",
        "user-agent",
        "x-csrftoken",
        "x-requested-with",
        "X-API-Key",
    ];
    pub const DEFAULT_ALLOW_METHODS: &'static [&'static str] =
        &["DELETE", "GET", "OPTIONS", "PATCH", "POST", "PUT"];
    pub const DEFAULT_PREFLIGHT_MAX_AGE: u64 = 86400;
}

/// Webhook SSRF allow/deny lists (ENV.md §1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSettings {
    /// `WEBHOOK_ALLOWED_IPS` parsed with `ipaddress.ip_network(cidr, strict=False)`;
    /// invalid entries are skipped with a warning.
    pub allowed_ips: Vec<IpNet>,
    /// `WEBHOOK_ALLOWED_HOSTS`, normalised to `strip().rstrip(".").lower()`.
    pub allowed_hosts: Vec<String>,
    /// `WEBHOOK_DISALLOWED_DOMAINS`, same normalisation.
    pub disallowed_domains: Vec<String>,
}

/// Primary and optional replica database configuration. The raw values are kept
/// here; `plane-db` implements `dj-database-url`'s parsing (ENV.md §1.3).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct PostgresParams {
    pub db: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseSettings {
    /// `DATABASE_URL` when non-empty (`bool(os.environ.get(...))`).
    pub url: Option<String>,
    /// `POSTGRES_*` used when the URL is empty or unset.
    pub params: PostgresParams,
    /// `ENABLE_READ_REPLICA == "1"`.
    pub enable_read_replica: bool,
    /// `DATABASE_READ_REPLICA_URL` when non-empty.
    pub replica_url: Option<String>,
    /// `POSTGRES_READ_REPLICA_*` used when the replica URL is empty.
    pub replica_params: PostgresParams,
}

/// Redis settings (ENV.md §1.4).
#[derive(Clone, PartialEq, Eq)]
pub struct RedisSettings {
    /// `REDIS_URL`; required by the roles that use Redis.
    pub url: Option<String>,
    /// `REDIS_SSL`: `env.get("REDIS_URL") and "rediss" in env.get("REDIS_URL")`.
    pub ssl: Option<bool>,
    /// `REDIS_SSL_CERT_REQS`, lower-cased with a `"required"` default.
    pub ssl_cert_reqs: String,
    /// `REDIS_SSL_CA_CERTS`, only when truthy.
    pub ssl_ca_certs: Option<String>,
}

/// The Celery broker URL (ENV.md §2.4) and the beat-side metrics interval
/// (§1.5).
#[derive(Clone, PartialEq, Eq)]
pub struct BrokerSettings {
    pub url: String,
    /// `METRICS_PUSH_INTERVAL_MINUTES`: `int(raw)`; valid only when
    /// `0 < value <= 10_000_000`, otherwise `360`. A malformed value (including
    /// the empty string) falls back to `360` rather than failing startup.
    pub metrics_push_interval_minutes: i64,
}

/// Object storage (ENV.md §1.6, §2.5, §5).
#[derive(Clone, PartialEq, Eq)]
pub struct StorageSettings {
    /// `settings.USE_MINIO`: `int(env.get("USE_MINIO", 0)) == 1`.
    pub use_minio_settings: bool,
    /// `S3Storage.USE_MINIO`: `env.get("USE_MINIO") == "1"`.
    pub use_minio_storage: bool,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    /// `settings.AWS_STORAGE_BUCKET_NAME`, default `"uploads"`.
    pub bucket_name: String,
    /// `storage.py` reads the same variable with **no** default.
    pub bucket_name_storage: Option<String>,
    /// `settings.AWS_REGION`, default `""`.
    pub region: String,
    /// `storage.py` reads `AWS_REGION` with no default.
    pub region_storage: Option<String>,
    /// `AWS_S3_ENDPOINT_URL or MINIO_ENDPOINT_URL`, each truthy.
    pub endpoint_url: Option<String>,
    pub aws_s3_endpoint_url: Option<String>,
    pub minio_endpoint_url: Option<String>,
    /// `MINIO_ENDPOINT_SSL == "1"`.
    pub minio_endpoint_ssl: bool,
    /// `SIGNED_URL_EXPIRATION` raw: `int()` runs at the first presign, not at
    /// import.
    pub signed_url_expiration_raw: String,
    /// `AWS_S3_CUSTOM_DOMAIN` (§2.5) when both the endpoint and MinIO are set.
    pub custom_domain: Option<String>,
    /// `AWS_S3_URL_PROTOCOL` (§2.5).
    pub url_protocol: Option<String>,
}

impl StorageSettings {
    /// `SIGNED_URL_EXPIRATION` as an integer, parsed where Django parses it.
    pub fn signed_url_expiration(&self) -> Result<i64, py::PyIntError> {
        py::int(&self.signed_url_expiration_raw)
    }
}

/// SMTP configuration (ENV.md §1.7). Read through `get_configuration_value`
/// at call time, so these raw values are the seed/fallback only.
#[derive(Clone, PartialEq, Eq)]
pub struct EmailSettings {
    pub host: Option<String>,
    pub host_user: Option<String>,
    pub host_password: Option<String>,
    /// Seeded as a string (`"587"`); the call-site fallback is the integer 587.
    pub port_raw: Option<String>,
    pub use_tls: bool,
    pub use_ssl: bool,
    pub from: Option<String>,
    /// Only `plane/settings/local.py` reads this.
    pub backend: String,
    /// Seeded only; never read for behaviour.
    pub enable_smtp: String,
}

impl EmailSettings {
    /// `int(EMAIL_PORT)` at a call site, with the int fallback `587`.
    pub fn port(&self) -> Result<i64, py::PyIntError> {
        match &self.port_raw {
            Some(raw) => py::int(raw),
            None => Ok(587),
        }
    }
}

/// URLs and paths (ENV.md §1.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlSettings {
    /// Raw `WEB_URL`: `None` unset, `Some("")` set-but-empty.
    pub web_url: Option<String>,
    /// `APP_BASE_URL`, validated with `is_valid_url`; invalid becomes `None`.
    pub app_base_url: Option<String>,
    pub app_base_path: String,
    pub admin_base_url: Option<String>,
    pub admin_base_path: String,
    pub space_base_url: Option<String>,
    pub space_base_path: String,
    pub live_base_url: Option<String>,
    pub live_base_path: String,
    /// `urljoin(LIVE_BASE_URL, LIVE_BASE_PATH)`; `None` without a base URL.
    pub live_url: Option<String>,
    pub instance_changelog_url: String,
}

/// Cookie and session settings (ENV.md §1.8, §2.1, HTTP_CONTRACT §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieSettings {
    pub session_cookie_name: String,
    pub session_cookie_age: i64,
    pub session_cookie_domain: Option<String>,
    pub session_cookie_secure: bool,
    pub session_cookie_httponly: bool,
    pub session_cookie_samesite: &'static str,
    pub session_cookie_path: &'static str,
    pub session_save_every_request: bool,
    pub admin_session_cookie_name: &'static str,
    pub admin_session_cookie_age: i64,
    pub csrf_cookie_name: &'static str,
    pub csrf_cookie_age: i64,
    pub csrf_cookie_secure: bool,
    pub csrf_cookie_httponly: bool,
    pub csrf_cookie_domain: Option<String>,
    pub csrf_cookie_samesite: &'static str,
    pub csrf_cookie_path: &'static str,
    pub csrf_header_name: &'static str,
    pub csrf_trusted_origins: Vec<String>,
}

/// Authentication and OAuth values (ENV.md §1.9). Every one of these is read
/// through `get_configuration_value()`, so the environment value is the seed and
/// fallback; the raw string is kept so call sites can apply their own defaults.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct AuthSettings {
    pub enable_signup: Option<String>,
    pub enable_email_password: Option<String>,
    pub enable_magic_link_login: Option<String>,
    pub is_google_enabled: Option<String>,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub enable_google_sync: Option<String>,
    pub is_github_enabled: Option<String>,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub github_organization_id: Option<String>,
    pub enable_github_sync: Option<String>,
    pub github_app_name: Option<String>,
    pub gitlab_client_id: Option<String>,
    pub gitlab_client_secret: Option<String>,
    pub gitlab_host: Option<String>,
    pub enable_gitlab_sync: Option<String>,
    pub gitea_client_id: Option<String>,
    pub gitea_client_secret: Option<String>,
    pub gitea_host: Option<String>,
    pub enable_gitea_sync: Option<String>,
    pub slack_client_id: Option<String>,
}

/// Limits and retention windows (ENV.md §1.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitsSettings {
    pub file_size_limit: i64,
    /// `DATA_UPLOAD_MAX_MEMORY_SIZE` is set from `FILE_SIZE_LIMIT` verbatim.
    pub data_upload_max_memory_size: i64,
    pub hard_delete_after_days: i64,
    pub api_activity_log_retention_days: i64,
    pub webhook_log_retention_days: i64,
    pub email_log_retention_days: i64,
    pub unuploaded_asset_delete_days: i64,
}

/// Rate limits and proxy identity (ENV.md §1.10, HTTP_CONTRACT §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThrottlingSettings {
    /// Raw `API_KEY_RATE_LIMIT`; DRF parses it on the first API-key request.
    pub api_key_rate_limit: String,
    /// Raw `AUTHENTICATION_RATE_LIMIT`.
    pub authentication_rate_limit: String,
    /// `REST_FRAMEWORK["NUM_PROXIES"]`: `None` when unset or invalid. Django
    /// accepts any integer (`int(raw)`) whose raw form is at most three decimal
    /// characters, so `999` is valid and is not clamped to `u8`.
    pub num_proxies: Option<u16>,
    /// `DEFAULT_THROTTLE_RATES`, hard-coded in `common.py:142-145`.
    pub anon_rate: &'static str,
    pub asset_rate: &'static str,
    /// `EmailVerificationThrottle`, hard-coded in `rate_limit.py:98`.
    pub email_verification_rate: &'static str,
}

/// Telemetry values (ENV.md §1.11, §2.7). Only the worker uses these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySettings {
    pub otlp_endpoint: String,
    pub otlp_metrics_protocol: String,
    pub otel_metrics_insecure: bool,
    pub service_name: String,
    /// Accepted and ignored: Scout is a Python agent.
    pub scout_monitor: Option<String>,
    pub scout_key: String,
}

/// Third-party integrations (ENV.md §1.13).
#[derive(Clone, PartialEq, Eq)]
pub struct IntegrationSettings {
    pub llm_api_key: Option<String>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub gpt_engine: Option<String>,
    pub unsplash_access_key: Option<String>,
    /// Dead setting kept for compatibility.
    pub github_access_token: Option<String>,
    /// Dead setting kept for compatibility.
    pub analytics_secret_key: Option<String>,
    /// Dead setting kept for compatibility.
    pub analytics_base_api: Option<String>,
    pub is_test: bool,
}

impl fmt::Debug for DatabaseSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseSettings")
            .field("url", &self.url.as_ref().map(|_| "<redacted>"))
            .field("params", &self.params)
            .field("enable_read_replica", &self.enable_read_replica)
            .field(
                "replica_url",
                &self.replica_url.as_ref().map(|_| "<redacted>"),
            )
            .field("replica_params", &self.replica_params)
            .finish()
    }
}

impl fmt::Debug for AuthSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthSettings")
            .field("enable_signup", &self.enable_signup)
            .field("enable_email_password", &self.enable_email_password)
            .field("enable_magic_link_login", &self.enable_magic_link_login)
            .field("is_google_enabled", &self.is_google_enabled)
            .field("google_client_id", &self.google_client_id)
            .field(
                "google_client_secret",
                &self.google_client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("is_github_enabled", &self.is_github_enabled)
            .field("github_client_id", &self.github_client_id)
            .field(
                "github_client_secret",
                &self.github_client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("github_organization_id", &self.github_organization_id)
            .field("github_app_name", &self.github_app_name)
            .field("gitlab_client_id", &self.gitlab_client_id)
            .field(
                "gitlab_client_secret",
                &self.gitlab_client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("gitlab_host", &self.gitlab_host)
            .field("gitea_client_id", &self.gitea_client_id)
            .field(
                "gitea_client_secret",
                &self.gitea_client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("gitea_host", &self.gitea_host)
            .field("slack_client_id", &self.slack_client_id)
            .finish_non_exhaustive()
    }
}

// Credential-bearing types get a redacting `Debug`: a `tracing::debug!` of a
// `Settings` value must never write a password to the logs (AGENTS.md rule 5).

impl fmt::Debug for PostgresParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresParams")
            .field("db", &self.db)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

impl fmt::Debug for RedisSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisSettings")
            .field("url", &self.url.as_ref().map(|_| "<redacted>"))
            .field("ssl", &self.ssl)
            .field("ssl_cert_reqs", &self.ssl_cert_reqs)
            .field("ssl_ca_certs", &self.ssl_ca_certs)
            .finish()
    }
}

impl fmt::Debug for BrokerSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrokerSettings")
            .field("url", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for StorageSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageSettings")
            .field("use_minio_settings", &self.use_minio_settings)
            .field("use_minio_storage", &self.use_minio_storage)
            .field(
                "access_key_id",
                &self.access_key_id.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "secret_access_key",
                &self.secret_access_key.as_ref().map(|_| "<redacted>"),
            )
            .field("bucket_name", &self.bucket_name)
            .field("region", &self.region)
            .field("endpoint_url", &self.endpoint_url)
            .field("custom_domain", &self.custom_domain)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for EmailSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailSettings")
            .field("host", &self.host)
            .field("host_user", &self.host_user)
            .field(
                "host_password",
                &self.host_password.as_ref().map(|_| "<redacted>"),
            )
            .field("port_raw", &self.port_raw)
            .field("use_tls", &self.use_tls)
            .field("use_ssl", &self.use_ssl)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for IntegrationSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntegrationSettings")
            .field(
                "llm_api_key",
                &self.llm_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("llm_provider", &self.llm_provider)
            .field("llm_model", &self.llm_model)
            .field(
                "unsplash_access_key",
                &self.unsplash_access_key.as_ref().map(|_| "<redacted>"),
            )
            .field("is_test", &self.is_test)
            .finish_non_exhaustive()
    }
}

/// `plane-core`-only knobs; every name is prefixed `PLANE_RS_` and documented
/// in ENV.md §6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustSettings {
    /// `PLANE_RS_BIND`, default `0.0.0.0:${PORT:-8000}` like the Django entrypoint.
    pub bind: SocketAddr,
    /// `PLANE_RS_WORKER_THREADS`; `None` (the default) leaves tokio's choice.
    pub worker_threads: Option<usize>,
    /// `PLANE_RS_DB_POOL_SIZE`.
    pub db_pool_size: usize,
    /// `PLANE_RS_LOG_FORMAT`.
    pub log_format: LogFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

impl LogFormat {
    /// Parse `PLANE_RS_LOG_FORMAT`: `json` (default), `pretty` or `compact`.
    pub fn parse_str(raw: &str) -> Result<Self, ConfigError> {
        match raw {
            "json" => Ok(LogFormat::Json),
            "pretty" => Ok(LogFormat::Pretty),
            "compact" => Ok(LogFormat::Compact),
            other => Err(ConfigError::new(
                "PLANE_RS_LOG_FORMAT",
                format!("expected one of json, pretty, compact; got {other:?}"),
            )),
        }
    }
}

/// Everything the `api` role reads from the environment (ENV.md §1).
#[derive(Debug, Clone)]
pub struct Settings {
    pub secret_key: SecretKey,
    pub debug: bool,
    pub runtime: DjangoRuntime,
    pub allowed_hosts: Vec<String>,
    pub cors: CorsSettings,
    pub webhook: WebhookSettings,
    pub skip_env_var: bool,
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    pub broker: BrokerSettings,
    pub storage: StorageSettings,
    pub email: EmailSettings,
    pub urls: UrlSettings,
    pub cookies: CookieSettings,
    pub auth: AuthSettings,
    pub limits: LimitsSettings,
    pub throttling: ThrottlingSettings,
    pub telemetry: TelemetrySettings,
    pub integrations: IntegrationSettings,
    pub rust: RustSettings,
    /// Non-fatal problems Django logs and continues from.
    pub warnings: Vec<ConfigWarning>,
}

/// Django runtime values (ENV.md §1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DjangoRuntime {
    /// Accepted for compatibility; Rust has no settings module.
    pub settings_module: Option<String>,
    pub enable_drf_spectacular: bool,
    pub app_version: Option<String>,
}

impl Settings {
    /// Load from the process environment for a given role.
    pub fn from_env(role: Role) -> Result<Self, ConfigError> {
        Self::from_source(&ProcessEnv, role)
    }

    /// Load from any [`EnvSource`], which is how the unit tests inject values.
    pub fn from_source<S: EnvSource>(source: &S, role: Role) -> Result<Self, ConfigError> {
        let mut env = EnvReader {
            source,
            warnings: Vec::new(),
        };

        // ---- §1.1 core and security -------------------------------------
        let secret_key = match env.raw("SECRET_KEY") {
            Some(value) if !value.is_empty() => SecretKey {
                value,
                random: false,
            },
            // `os.environ.get("SECRET_KEY") or get_random_secret_key()`:
            // empty counts as unset (GHSA-cmwv-pjmw-8483).
            _ => SecretKey {
                value: random_secret_key(),
                random: true,
            },
        };
        if secret_key.value == "60gp0byfz2dvffa45cxl20p1scy9xbpf6d8c5y0geejgkyp1b5"
            || secret_key.value == "change-this-key-on-deployment"
        {
            // Same critical message as `plane/settings/common.py:38-46`.
            tracing::error!(
                target: "plane",
                severity = "CRITICAL",
                "SECURITY: SECRET_KEY is set to a known insecure or placeholder value. \
                 This makes your installation vulnerable to session forgery, CSRF bypass, and \
                 password-reset token forging. Set a unique SECRET_KEY before deploying to production."
            );
        }

        // `DEBUG` is int()-parsed twice in production; the first is common.py:52.
        let debug = env.int("DEBUG", 0)? == 1;

        let allowed_hosts: Vec<String> = py::split_comma(&env.get_str("ALLOWED_HOSTS", "*"))
            .into_iter()
            .map(str::to_string)
            .collect();

        let cors_allowed_origins = parse_origin_list(&env.get_str("CORS_ALLOWED_ORIGINS", ""));
        let cors = CorsSettings {
            allow_all_origins: cors_allowed_origins.is_empty(),
            allowed_origins: cors_allowed_origins,
            allow_credentials: true,
            allow_headers: CorsSettings::DEFAULT_ALLOW_HEADERS.to_vec(),
            allow_methods: CorsSettings::DEFAULT_ALLOW_METHODS.to_vec(),
            preflight_max_age: CorsSettings::DEFAULT_PREFLIGHT_MAX_AGE,
            expose_headers: Vec::new(),
            allow_private_network: false,
            urls_regex_all: true,
        };

        // `COOKIE_DOMAIN` passes through unchanged; `or None` in the settings.
        let cookie_domain = env.raw("COOKIE_DOMAIN").filter(|value| !value.is_empty());

        let webhook = WebhookSettings {
            allowed_ips: parse_networks(&mut env, "WEBHOOK_ALLOWED_IPS"),
            allowed_hosts: parse_host_list(&env.get_str("WEBHOOK_ALLOWED_HOSTS", "")),
            disallowed_domains: parse_host_list(&env.get_str("WEBHOOK_DISALLOWED_DOMAINS", "")),
        };

        let skip_env_var = env.flag("SKIP_ENV_VAR", "1");

        // ---- §1.2 Django runtime ----------------------------------------
        let runtime = DjangoRuntime {
            settings_module: env.raw("DJANGO_SETTINGS_MODULE"),
            enable_drf_spectacular: env.flag("ENABLE_DRF_SPECTACULAR", "0"),
            app_version: env.raw("APP_VERSION"),
        };

        // ---- §1.3 database ----------------------------------------------
        let database = DatabaseSettings {
            url: env.raw("DATABASE_URL").filter(|value| !value.is_empty()),
            params: PostgresParams {
                db: env.raw("POSTGRES_DB"),
                user: env.raw("POSTGRES_USER"),
                password: env.raw("POSTGRES_PASSWORD"),
                host: env.raw("POSTGRES_HOST"),
                port: env.get_str("POSTGRES_PORT", "5432"),
            },
            enable_read_replica: env.flag("ENABLE_READ_REPLICA", "0"),
            replica_url: env
                .raw("DATABASE_READ_REPLICA_URL")
                .filter(|value| !value.is_empty()),
            replica_params: PostgresParams {
                db: env.raw("POSTGRES_READ_REPLICA_DB"),
                user: env.raw("POSTGRES_READ_REPLICA_USER"),
                password: env.raw("POSTGRES_READ_REPLICA_PASSWORD"),
                host: env.raw("POSTGRES_READ_REPLICA_HOST"),
                port: env.get_str("POSTGRES_READ_REPLICA_PORT", "5432"),
            },
        };

        // ---- §1.4 Redis --------------------------------------------------
        let redis_url = env.raw("REDIS_URL");
        let redis = RedisSettings {
            // `REDIS_SSL = REDIS_URL and "rediss" in REDIS_URL`: None, False or True.
            ssl: redis_url.as_deref().map(|url| url.contains("rediss")),
            url: redis_url.filter(|value| !value.is_empty()),
            ssl_cert_reqs: {
                let from_url = env
                    .raw("REDIS_URL")
                    .and_then(|url| {
                        let split = py::urlsplit(&url).ok()?;
                        py::parse_qs(&split.query)
                            .into_iter()
                            .find(|(key, _)| key == "ssl_cert_reqs")
                            .and_then(|(_, mut values)| values.pop())
                    })
                    .unwrap_or_default();
                let value = if from_url.trim().is_empty() {
                    env.get_str("REDIS_SSL_CERT_REQS", "")
                } else {
                    from_url
                };
                let value = value.trim().to_lowercase();
                if value.is_empty() {
                    "required".to_string()
                } else {
                    value
                }
            },
            ssl_ca_certs: env
                .raw("REDIS_SSL_CA_CERTS")
                .filter(|value| !value.is_empty()),
        };
        if role.needs_redis() && redis.url.is_none() {
            // Django aborts every role at import with
            // `AttributeError: 'NoneType' object has no attribute 'startswith'`.
            return Err(ConfigError::new(
                "REDIS_URL",
                "REDIS_URL is required for the api, worker and beat roles \
                 (Django fails while importing plane.celery)",
            ));
        }

        // ---- §1.5 RabbitMQ / Celery -------------------------------------
        let metrics_push_interval_minutes = match env.raw("METRICS_PUSH_INTERVAL_MINUTES") {
            Some(raw) => match py::int(&raw) {
                Ok(value) if value > 0 && value <= 10_000_000 => value,
                _ => 360,
            },
            None => 360,
        };
        let broker = BrokerSettings {
            url: match env.raw("AMQP_URL").filter(|value| !value.is_empty()) {
                Some(url) => url,
                None => format!(
                    "amqp://{}:{}@{}:{}/{}",
                    env.get_str("RABBITMQ_USER", "guest"),
                    env.get_str("RABBITMQ_PASSWORD", "guest"),
                    env.get_str("RABBITMQ_HOST", "localhost"),
                    env.get_str("RABBITMQ_PORT", "5672"),
                    env.get_str("RABBITMQ_VHOST", "/"),
                ),
            },
            metrics_push_interval_minutes,
        };

        // ---- §1.6 object storage ----------------------------------------
        let aws_s3_endpoint_url = env.raw("AWS_S3_ENDPOINT_URL").filter(|v| !v.is_empty());
        let minio_endpoint_url = env.raw("MINIO_ENDPOINT_URL").filter(|v| !v.is_empty());
        let endpoint_url = aws_s3_endpoint_url
            .clone()
            .or_else(|| minio_endpoint_url.clone());
        let use_minio_settings = env.int("USE_MINIO", 0)? == 1;
        let use_minio_storage = env.get_str("USE_MINIO", "") == "1";
        let bucket_name = env.get_str("AWS_S3_BUCKET_NAME", "uploads");
        let (custom_domain, url_protocol) = if endpoint_url.is_some() && use_minio_settings {
            // `urlparse(env.get("WEB_URL", "http://localhost"))` — note that an
            // empty WEB_URL is *not* replaced, producing a leading-slash domain.
            let web_url = env.get_str("WEB_URL", "http://localhost");
            match py::urlsplit(&web_url) {
                Ok(split) => (
                    Some(format!("{}/{}", split.netloc, bucket_name)),
                    Some(format!("{}:", split.scheme)),
                ),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        };
        let storage = StorageSettings {
            use_minio_settings,
            use_minio_storage,
            access_key_id: env.raw("AWS_ACCESS_KEY_ID"),
            secret_access_key: env.raw("AWS_SECRET_ACCESS_KEY"),
            bucket_name: bucket_name.clone(),
            bucket_name_storage: env.raw("AWS_S3_BUCKET_NAME"),
            region: env.get_str("AWS_REGION", ""),
            region_storage: env.raw("AWS_REGION"),
            endpoint_url,
            aws_s3_endpoint_url,
            minio_endpoint_url,
            minio_endpoint_ssl: env.flag("MINIO_ENDPOINT_SSL", "0"),
            signed_url_expiration_raw: env.get_str("SIGNED_URL_EXPIRATION", "3600"),
            custom_domain,
            url_protocol,
        };
        if storage.use_minio_settings && storage.secret_access_key.as_deref() == Some("secret-key")
        {
            tracing::error!(
                target: "plane",
                severity = "CRITICAL",
                "SECURITY: MinIO is enabled with the public placeholder credentials \
                 (AWS_SECRET_ACCESS_KEY=secret-key); change them before deploying."
            );
        }

        // ---- §1.7 email --------------------------------------------------
        let email = EmailSettings {
            host: env.raw("EMAIL_HOST"),
            host_user: env.raw("EMAIL_HOST_USER"),
            host_password: env.raw("EMAIL_HOST_PASSWORD"),
            port_raw: env.raw("EMAIL_PORT"),
            use_tls: env.flag("EMAIL_USE_TLS", "1"),
            use_ssl: env.flag("EMAIL_USE_SSL", "0"),
            from: env.raw("EMAIL_FROM"),
            backend: env.get_str(
                "EMAIL_BACKEND",
                "django.core.mail.backends.console.EmailBackend",
            ),
            enable_smtp: env.get_str("ENABLE_SMTP", "0"),
        };

        // ---- §1.8 URLs, paths and cookies -------------------------------
        let web_url = env.raw("WEB_URL");
        let app_base_url = validated_url(&env.raw("APP_BASE_URL"));
        let admin_base_url = validated_url(&env.raw("ADMIN_BASE_URL"));
        let space_base_url = validated_url(&env.raw("SPACE_BASE_URL"));
        let live_base_url = validated_url(&env.raw("LIVE_BASE_URL"));
        let live_base_path = env.get_str("LIVE_BASE_PATH", "/live/");
        let live_url = live_base_url
            .as_deref()
            .map(|base| urljoin(base, &live_base_path));
        let urls = UrlSettings {
            web_url: web_url.clone(),
            app_base_url,
            app_base_path: env.get_str("APP_BASE_PATH", "/"),
            admin_base_url,
            admin_base_path: env.get_str("ADMIN_BASE_PATH", "/god-mode/"),
            space_base_url,
            space_base_path: env.get_str("SPACE_BASE_PATH", "/spaces/"),
            live_base_url,
            live_base_path,
            live_url,
            instance_changelog_url: env.get_str("INSTANCE_CHANGELOG_URL", ""),
        };

        // `_cookies_secure(env("WEB_URL"), cors_allowed_origins,
        //                  app_base_url=env("APP_BASE_URL"))` — the *raw*
        // environment values, before `is_valid_url` validation (§2.1).
        let session_cookie_secure = cookies_secure(
            web_url.as_deref(),
            &cors.allowed_origins,
            env.raw("APP_BASE_URL").as_deref(),
        );
        let session_cookie_age = env.int("SESSION_COOKIE_AGE", 604800)?;
        let admin_session_cookie_age = env.int("ADMIN_SESSION_COOKIE_AGE", 3600)?;
        let cookies = CookieSettings {
            session_cookie_name: env.get_str("SESSION_COOKIE_NAME", "session-id"),
            session_cookie_age,
            session_cookie_domain: cookie_domain.clone(),
            session_cookie_secure,
            session_cookie_httponly: true,
            session_cookie_samesite: "Lax",
            session_cookie_path: "/",
            session_save_every_request: env.flag("SESSION_SAVE_EVERY_REQUEST", "0"),
            admin_session_cookie_name: "admin-session-id",
            admin_session_cookie_age,
            csrf_cookie_name: "csrftoken",
            csrf_cookie_age: 31449600,
            csrf_cookie_secure: session_cookie_secure,
            csrf_cookie_httponly: true,
            csrf_cookie_domain: cookie_domain,
            csrf_cookie_samesite: "Lax",
            csrf_cookie_path: "/",
            csrf_header_name: "X-CSRFToken",
            csrf_trusted_origins: cors.allowed_origins.clone(),
        };

        // ---- §1.9 authentication and OAuth ------------------------------
        let auth = AuthSettings {
            enable_signup: env.raw("ENABLE_SIGNUP"),
            enable_email_password: env.raw("ENABLE_EMAIL_PASSWORD"),
            enable_magic_link_login: env.raw("ENABLE_MAGIC_LINK_LOGIN"),
            is_google_enabled: env.raw("IS_GOOGLE_ENABLED"),
            google_client_id: env.raw("GOOGLE_CLIENT_ID"),
            google_client_secret: env.raw("GOOGLE_CLIENT_SECRET"),
            enable_google_sync: env.raw("ENABLE_GOOGLE_SYNC"),
            is_github_enabled: env.raw("IS_GITHUB_ENABLED"),
            github_client_id: env.raw("GITHUB_CLIENT_ID"),
            github_client_secret: env.raw("GITHUB_CLIENT_SECRET"),
            github_organization_id: env.raw("GITHUB_ORGANIZATION_ID"),
            enable_github_sync: env.raw("ENABLE_GITHUB_SYNC"),
            github_app_name: env.raw("GITHUB_APP_NAME"),
            gitlab_client_id: env.raw("GITLAB_CLIENT_ID"),
            gitlab_client_secret: env.raw("GITLAB_CLIENT_SECRET"),
            gitlab_host: env.raw("GITLAB_HOST"),
            enable_gitlab_sync: env.raw("ENABLE_GITLAB_SYNC"),
            gitea_client_id: env.raw("GITEA_CLIENT_ID"),
            gitea_client_secret: env.raw("GITEA_CLIENT_SECRET"),
            gitea_host: env.raw("GITEA_HOST"),
            enable_gitea_sync: env.raw("ENABLE_GITEA_SYNC"),
            slack_client_id: env.raw("SLACK_CLIENT_ID"),
        };

        // ---- §1.10 rate limits and client IP ----------------------------
        let num_proxies = match env.get_str("NUM_PROXIES", "").trim() {
            "" => None,
            raw if py::is_decimal(raw) && raw.chars().count() <= 3 => {
                match py::int(raw) {
                    // 0..=999 by construction (three decimal characters).
                    Ok(value) if (0..=999).contains(&value) => Some(value as u16),
                    _ => {
                        // `int()` cannot fail here for ASCII decimals of length
                        // <= 3; guard anyway so a future table change cannot
                        // turn a warning into a panic.
                        env.warning("NUM_PROXIES", format!("ignoring invalid value {raw:?}"));
                        None
                    }
                }
            }
            raw => {
                env.warning("NUM_PROXIES", format!("ignoring invalid value {raw:?}"));
                None
            }
        };
        let throttling = ThrottlingSettings {
            api_key_rate_limit: env.get_str("API_KEY_RATE_LIMIT", "60/minute"),
            authentication_rate_limit: env.get_str("AUTHENTICATION_RATE_LIMIT", "10/minute"),
            num_proxies,
            anon_rate: "30/minute",
            asset_rate: "5/minute",
            email_verification_rate: "3/hour",
        };

        // ---- §1.11 logging and telemetry -------------------------------
        let telemetry = TelemetrySettings {
            otlp_endpoint: env.get_str("OTLP_ENDPOINT", "https://telemetry.plane.so"),
            otlp_metrics_protocol: env
                .or("OTLP_METRICS_PROTOCOL", "grpc")
                .trim()
                .to_lowercase(),
            otel_metrics_insecure: env
                .get_str("OTEL_EXPORTER_OTLP_METRICS_INSECURE", "")
                .to_lowercase()
                == "true",
            service_name: env.get_str("SERVICE_NAME", "plane-ce-api"),
            scout_monitor: env.raw("SCOUT_MONITOR").filter(|value| !value.is_empty()),
            scout_key: env.get_str("SCOUT_KEY", ""),
        };

        // ---- §1.12 limits and retention --------------------------------
        let file_size_limit = env.int("FILE_SIZE_LIMIT", 5242880)?;
        let limits = LimitsSettings {
            file_size_limit,
            data_upload_max_memory_size: file_size_limit,
            hard_delete_after_days: env.int("HARD_DELETE_AFTER_DAYS", 60)?,
            api_activity_log_retention_days: py::retention_days(
                env.raw("API_ACTIVITY_LOG_RETENTION_DAYS").as_deref(),
                14,
            ),
            webhook_log_retention_days: py::retention_days(
                env.raw("WEBHOOK_LOG_RETENTION_DAYS").as_deref(),
                14,
            ),
            email_log_retention_days: py::retention_days(
                env.raw("EMAIL_LOG_RETENTION_DAYS").as_deref(),
                7,
            ),
            unuploaded_asset_delete_days: env.int("UNUPLOADED_ASSET_DELETE_DAYS", 7)?,
        };

        // ---- §1.13 integrations ----------------------------------------
        let integrations = IntegrationSettings {
            llm_api_key: env.raw("LLM_API_KEY"),
            llm_provider: env.raw("LLM_PROVIDER"),
            llm_model: env.raw("LLM_MODEL"),
            gpt_engine: env.raw("GPT_ENGINE"),
            unsplash_access_key: env.raw("UNSPLASH_ACCESS_KEY"),
            github_access_token: env.raw("GITHUB_ACCESS_TOKEN"),
            analytics_secret_key: env.raw("ANALYTICS_SECRET_KEY"),
            analytics_base_api: env.raw("ANALYTICS_BASE_API"),
            is_test: env.flag("IS_TEST", "0"),
        };

        // ---- §6 Rust-only knobs ----------------------------------------
        let bind = match env.raw("PLANE_RS_BIND") {
            Some(value) => value.parse::<SocketAddr>().map_err(|error| {
                ConfigError::new(
                    "PLANE_RS_BIND",
                    format!("invalid socket address {value:?}: {error}"),
                )
            })?,
            None => {
                // Default mirrors `bin/docker-entrypoint-api.sh`: `0.0.0.0:${PORT:-8000}`.
                let port = env.get_str("PORT", "8000");
                let port: u16 = port
                    .parse()
                    .map_err(|_| ConfigError::new("PORT", format!("invalid port {port:?}")))?;
                SocketAddr::from(([0, 0, 0, 0], port))
            }
        };
        let worker_threads = match env.raw("PLANE_RS_WORKER_THREADS") {
            None => None,
            Some(raw) => match py::int(&raw) {
                Ok(0) => None,
                Ok(value) if value > 0 && value <= 4096 => Some(value as usize),
                _ => {
                    return Err(ConfigError::new(
                        "PLANE_RS_WORKER_THREADS",
                        format!("expected a positive integer (0 = automatic), got {raw:?}"),
                    ));
                }
            },
        };
        let db_pool_size = match env.raw("PLANE_RS_DB_POOL_SIZE") {
            None => 16,
            Some(raw) => match py::int(&raw) {
                Ok(value) if value > 0 && value <= 1000 => value as usize,
                _ => {
                    return Err(ConfigError::new(
                        "PLANE_RS_DB_POOL_SIZE",
                        format!("expected a positive integer, got {raw:?}"),
                    ));
                }
            },
        };
        let log_format = LogFormat::parse_str(&env.get_str("PLANE_RS_LOG_FORMAT", "json"))?;
        let rust = RustSettings {
            bind,
            worker_threads,
            db_pool_size,
            log_format,
        };

        Ok(Settings {
            secret_key,
            debug,
            runtime,
            allowed_hosts,
            cors,
            webhook,
            skip_env_var,
            database,
            redis,
            broker,
            storage,
            email,
            urls,
            cookies,
            auth,
            limits,
            throttling,
            telemetry,
            integrations,
            rust,
            warnings: env.warnings,
        })
    }
}

// ---------------------------------------------------------------------------
// Derivation helpers (each maps to one §2 rule)
// ---------------------------------------------------------------------------

/// `common.py:406` `_cookies_secure` (ENV.md §2.1).
pub fn cookies_secure(
    web_url: Option<&str>,
    cors_allowed_origins: &[String],
    app_base_url: Option<&str>,
) -> bool {
    let base = web_url.filter(|value| !value.is_empty()).unwrap_or("");
    let base = if base.is_empty() {
        app_base_url.unwrap_or("")
    } else {
        base
    };
    if base.trim().to_lowercase().starts_with("https://") {
        return true;
    }
    !cors_allowed_origins.is_empty()
        && !cors_allowed_origins
            .iter()
            .any(|origin| origin.contains("http:"))
}

/// `is_valid_url(value) ? Some(value) : None`, without normalising the value
/// (ENV.md §1.8).
fn validated_url(value: &Option<String>) -> Option<String> {
    match value {
        Some(value) if py::is_valid_url(value) => Some(value.clone()),
        _ => None,
    }
}

/// `CORS_ALLOWED_ORIGINS`: split on `,`, strip each entry, drop empties.
fn parse_origin_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(str::to_string)
        .collect()
}

/// `WEBHOOK_ALLOWED_HOSTS`/`WEBHOOK_DISALLOWED_DOMAINS`: keep non-empty
/// entries as `strip().rstrip(".").lower()`.
fn parse_host_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| entry.trim().trim_end_matches('.').to_lowercase())
        .collect()
}

/// `WEBHOOK_ALLOWED_IPS`: `ip_network(cidr, strict=False)`; invalid entries are
/// logged at WARNING and skipped (never fatal).
fn parse_networks<S: EnvSource>(env: &mut EnvReader<'_, S>, variable: &str) -> Vec<IpNet> {
    let raw = env.get_str(variable, "");
    let mut networks = Vec::new();
    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        match entry.parse::<IpNet>() {
            Ok(network) => networks.push(network.trunc()),
            Err(error) => env.warning(
                variable,
                format!("ignoring invalid network {entry:?}: {error}"),
            ),
        }
    }
    networks
}

/// `django.utils.http.urljoin` for an absolute base and a path.
fn urljoin(base: &str, path: &str) -> String {
    match url::Url::parse(base.trim()) {
        Ok(base_url) => base_url
            .join(path)
            .map(|joined| joined.to_string())
            .unwrap_or_else(|_| path.to_string()),
        Err(_) => path.to_string(),
    }
}

/// `django.core.management.utils.get_random_secret_key()`: 50 characters from
/// `abcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*(-_=+)`.
fn random_secret_key() -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*(-_=+)";
    let mut bytes = [0u8; 50];
    // A failure of the system random source is not recoverable; Django's
    // `get_random_secret_key` would raise at import too.
    if getrandom::fill(&mut bytes).is_err() {
        // Extremely unlikely; fall back to a deterministic-but-unusual value is
        // worse than failing, so panic with a clear message.
        panic!("the system random source failed while generating SECRET_KEY");
    }
    bytes
        .iter()
        .map(|byte| CHARS[(*byte as usize) % CHARS.len()] as char)
        .collect()
}

#[cfg(test)]
mod tests;
