// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `dj-database-url` parsing, reproduced from the pinned 3.0.1 sources.
//!
//! `plane/settings/common.py:207-232` builds `DATABASES["default"]` (and the
//! optional `replica`) either from `DATABASE_URL`/`DATABASE_READ_REPLICA_URL` or
//! from the five `POSTGRES_*` variables. `dj_database_url.parse()` uses
//! `urllib.parse.urlsplit`: the scheme is checked against a fixed list, the
//! user/password/host/path components are percent-decoded, the port is parsed
//! by `SplitResult.port`, and the query string becomes Django's `OPTIONS` with
//! a per-item conversion (`_parse_value`). A blank query value is dropped
//! (`parse_qs` defaults to `keep_blank_values=False`).
//!
//! The port keeps `OPTIONS` as a typed value so `PostgresSettings::apply`
//! can map the ones tokio-postgres understands and report the rest.

use std::collections::BTreeMap;

use plane_core::config::{DatabaseSettings, PostgresParams};
use plane_core::py;

/// The schemes `dj_database_url` 3.0.1 registers for Postgres.
const POSTGRES_SCHEMES: &[&str] = &[
    "postgres",
    "postgresql",
    "pgsql",
    "postgis",
    "redshift",
    "timescale",
    "timescalegis",
];

/// The engine string the Postgres schemes map to.
pub const POSTGRES_ENGINE: &str = "django.db.backends.postgresql";

/// One `OPTIONS` value, typed the way `_parse_value` types it.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionValue {
    Int(i64),
    Bool(bool),
    Str(String),
    List(Vec<OptionValue>),
}

impl OptionValue {
    /// The scalar as a string, if it is scalar.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    /// The scalar as an integer, if it is one.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// The scalar as a bool, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }
}

/// Why parsing a database URL failed (Django raises `ParseError` or
/// `UnknownSchemeError` at settings-import time).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DatabaseUrlError {
    #[error(
        "This string is not a valid url, possibly because some of its parts is not properly urllib.parse.quote()'ed."
    )]
    Parse,
    #[error("Scheme '{0}://' is unknown")]
    UnknownScheme(String),
    #[error("the port is not a valid integer between 0 and 65535")]
    InvalidPort,
}

/// The `DATABASES[...]` dictionary for one alias.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseConfig {
    pub engine: &'static str,
    /// `NAME`: `""` when the URL has no path, `None` for an unset discrete variable.
    pub name: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    /// `PORT`: a string, as Django stores it; `""` when the URL has no port.
    pub port: String,
    pub options: BTreeMap<String, OptionValue>,
}

impl DatabaseConfig {
    /// `dj_database_url.parse(url)` for one of the Postgres schemes.
    pub fn parse(url: &str) -> Result<Self, DatabaseUrlError> {
        let split = py::urlsplit(url).map_err(|_| DatabaseUrlError::Parse)?;
        if !POSTGRES_SCHEMES.contains(&split.scheme.as_str()) {
            return Err(DatabaseUrlError::UnknownScheme(split.scheme));
        }
        let path = split.path.strip_prefix('/').unwrap_or(&split.path);
        let mut options = BTreeMap::new();
        for (key, values) in py::parse_qs(&split.query) {
            let parsed: Vec<OptionValue> = values
                .iter()
                .map(|value| parse_option_value(value))
                .collect::<Result<_, _>>()?;
            let value = if parsed.len() == 1 {
                parsed
                    .into_iter()
                    .next()
                    .unwrap_or(OptionValue::Str(String::new()))
            } else {
                OptionValue::List(parsed)
            };
            options.insert(key, value);
        }
        let port = match split.port() {
            Ok(Some(port)) => port.to_string(),
            Ok(None) => String::new(),
            Err(_) => return Err(DatabaseUrlError::InvalidPort),
        };

        // `apply_current_schema`: a `currentSchema` option becomes
        // `options=-c search_path=<schema>` and is removed.
        if let Some(schema) = options
            .remove("currentSchema")
            .as_ref()
            .and_then(OptionValue::as_str)
            .filter(|schema| !schema.is_empty())
        {
            options.insert(
                "options".to_string(),
                OptionValue::Str(format!("-c search_path={schema}")),
            );
        }

        Ok(Self {
            engine: POSTGRES_ENGINE,
            name: Some(py::unquote(path)),
            user: Some(py::unquote(split.username().unwrap_or(""))),
            password: Some(py::unquote(split.password().unwrap_or(""))),
            host: Some(py::unquote(&split.hostname().unwrap_or_default())),
            port,
            options,
        })
    }

    /// The `else` branch of `common.py:207`: the five discrete variables,
    /// with `NAME`/`USER`/`PASSWORD`/`HOST` left as `None` when unset and
    /// `PORT` defaulting to `"5432"`.
    pub fn from_params(params: &PostgresParams) -> Self {
        Self {
            engine: POSTGRES_ENGINE,
            name: params.db.clone(),
            user: params.user.clone(),
            password: params.password.clone(),
            host: params.host.clone(),
            port: params.port.clone(),
            options: BTreeMap::new(),
        }
    }
}

/// `_parse_value`: all-digits → int, `true`/`false` case-insensitively → bool,
/// else the string.
fn parse_option_value(value: &str) -> Result<OptionValue, DatabaseUrlError> {
    if py::is_decimal(value) {
        return match py::int(value) {
            Ok(value) => Ok(OptionValue::Int(value)),
            Err(_) => Err(DatabaseUrlError::Parse),
        };
    }
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(OptionValue::Bool(true)),
        "false" => Ok(OptionValue::Bool(false)),
        _ => Ok(OptionValue::Str(value.to_string())),
    }
}

/// The primary and optional replica configs derived from `Settings`
/// (`common.py:207-245`).
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseConfigs {
    pub primary: DatabaseConfig,
    pub replica: Option<DatabaseConfig>,
}

impl DatabaseConfigs {
    pub fn from_settings(settings: &DatabaseSettings) -> Result<Self, DatabaseUrlError> {
        let primary = match &settings.url {
            Some(url) => DatabaseConfig::parse(url)?,
            None => DatabaseConfig::from_params(&settings.params),
        };
        // `common.py:228`: the replica alias only exists when
        // ENABLE_READ_REPLICA == "1"; a replica configured while disabled is
        // ignored exactly as Django ignores it.
        let replica = if settings.enable_read_replica {
            Some(match &settings.replica_url {
                Some(url) => DatabaseConfig::parse(url)?,
                None => DatabaseConfig::from_params(&settings.replica_params),
            })
        } else {
            None
        };
        Ok(Self { primary, replica })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ENV.md §1.3: the documented example URL.
    #[test]
    fn parses_the_documented_example() {
        let config =
            DatabaseConfig::parse("postgresql://plane:plane@plane-db:5432/plane?sslmode=require")
                .unwrap();
        assert_eq!(config.engine, POSTGRES_ENGINE);
        assert_eq!(config.name.as_deref(), Some("plane"));
        assert_eq!(config.user.as_deref(), Some("plane"));
        assert_eq!(config.password.as_deref(), Some("plane"));
        assert_eq!(config.host.as_deref(), Some("plane-db"));
        assert_eq!(config.port, "5432");
        assert_eq!(
            config.options.get("sslmode"),
            Some(&OptionValue::Str("require".to_string()))
        );
    }

    // ENV.md §1.3: percent-decoding of USER/PASSWORD/HOST/NAME; the port is
    // `urlsplit().port`; a missing port is "".
    #[test]
    fn percent_decoding_and_ports() {
        let config = DatabaseConfig::parse("postgres://u%40x:p%3Aw@h.example/db%2Fname").unwrap();
        assert_eq!(config.user.as_deref(), Some("u@x"));
        assert_eq!(config.password.as_deref(), Some("p:w"));
        assert_eq!(config.name.as_deref(), Some("db/name"));
        assert_eq!(config.port, "");

        let config = DatabaseConfig::parse("postgres://h:1/db").unwrap();
        assert_eq!(config.port, "1");

        // `parse` maps a bad port to ParseError (dj_database_url catches the
        // ValueError raised by SplitResult.port).
        assert!(DatabaseConfig::parse("postgres://h:abc/db").is_err());
        assert!(DatabaseConfig::parse("postgres://h:70000/db").is_err());
    }

    // ENV.md §1.3: unknown schemes raise at startup.
    #[test]
    fn scheme_validation() {
        for scheme in POSTGRES_SCHEMES {
            assert!(
                DatabaseConfig::parse(&format!("{scheme}://h/db")).is_ok(),
                "{scheme}"
            );
        }
        assert_eq!(
            DatabaseConfig::parse("mysql://h/db"),
            Err(DatabaseUrlError::UnknownScheme("mysql".to_string()))
        );
        // No scheme at all parses as scheme "" and is unknown.
        assert_eq!(
            DatabaseConfig::parse("localhost:5432/db"),
            Err(DatabaseUrlError::UnknownScheme("localhost".to_string()))
        );
    }

    // ENV.md §1.3/§5: the query string becomes OPTIONS with `_parse_value`
    // conversions; repeated keys become a list; blank values are dropped.
    #[test]
    fn options_typing() {
        let config = DatabaseConfig::parse(
            "postgres://h/db?ssl=true&connect_timeout=10&application_name=plane&opt=&multi=a&multi=b",
        )
        .unwrap();
        assert_eq!(config.options.get("ssl"), Some(&OptionValue::Bool(true)));
        assert_eq!(
            config.options.get("connect_timeout"),
            Some(&OptionValue::Int(10))
        );
        assert_eq!(
            config.options.get("application_name"),
            Some(&OptionValue::Str("plane".to_string()))
        );
        assert!(!config.options.contains_key("opt"));
        assert_eq!(
            config.options.get("multi"),
            Some(&OptionValue::List(vec![
                OptionValue::Str("a".to_string()),
                OptionValue::Str("b".to_string())
            ]))
        );
        // "TRUE" is case-insensitive; anything else stays a string.
        let config = DatabaseConfig::parse("postgres://h/db?x=TRUE&y=yes&z=-1").unwrap();
        assert_eq!(config.options.get("x"), Some(&OptionValue::Bool(true)));
        assert_eq!(
            config.options.get("y"),
            Some(&OptionValue::Str("yes".into()))
        );
        assert_eq!(
            config.options.get("z"),
            Some(&OptionValue::Str("-1".into()))
        );
    }

    // ENV.md §1.3: `currentSchema` is replaced by `options=-c search_path=…`.
    #[test]
    fn current_schema_becomes_options() {
        let config = DatabaseConfig::parse("postgres://h/db?currentSchema=plane").unwrap();
        assert!(!config.options.contains_key("currentSchema"));
        assert_eq!(
            config.options.get("options"),
            Some(&OptionValue::Str("-c search_path=plane".to_string()))
        );

        // An empty currentSchema is dropped without adding options.
        let config = DatabaseConfig::parse("postgres://h/db?currentSchema=").unwrap();
        assert!(!config.options.contains_key("options"));
    }

    // ENV.md §1.3: set-but-empty DATABASE_URL falls back to POSTGRES_*, whose
    // unset values stay None and whose port defaults to "5432".
    #[test]
    fn discrete_variables() {
        let params = PostgresParams::default();
        let config = DatabaseConfig::from_params(&params);
        assert_eq!(config.name, None);
        assert_eq!(config.user, None);
        assert_eq!(config.host, None);
        assert_eq!(config.port, "");

        let settings = DatabaseSettings {
            url: None,
            params: PostgresParams {
                db: Some("plane".into()),
                user: Some("plane".into()),
                password: Some("".into()),
                host: Some("".into()),
                port: "5432".into(),
            },
            enable_read_replica: false,
            replica_url: None,
            replica_params: PostgresParams::default(),
        };
        let configs = DatabaseConfigs::from_settings(&settings).unwrap();
        assert_eq!(configs.primary.name.as_deref(), Some("plane"));
        assert_eq!(configs.primary.password.as_deref(), Some(""));
        assert_eq!(configs.primary.host.as_deref(), Some(""));
        assert_eq!(configs.primary.port, "5432");
        assert_eq!(configs.replica, None);
    }

    // ENV.md §1.3: the replica alias exists only when ENABLE_READ_REPLICA=1,
    // and it prefers its URL.
    #[test]
    fn replica_selection() {
        let mut settings = DatabaseSettings {
            url: Some("postgres://p:p@primary/db".into()),
            params: PostgresParams::default(),
            enable_read_replica: true,
            replica_url: Some("postgres://r:r@replica/rdb".into()),
            replica_params: PostgresParams::default(),
        };
        let configs = DatabaseConfigs::from_settings(&settings).unwrap();
        assert_eq!(configs.primary.host.as_deref(), Some("primary"));
        assert_eq!(
            configs.replica.as_ref().unwrap().host.as_deref(),
            Some("replica")
        );

        // Disabled: a replica URL is ignored.
        settings.enable_read_replica = false;
        assert_eq!(
            DatabaseConfigs::from_settings(&settings).unwrap().replica,
            None
        );

        // Enabled but no URL: the discrete replica variables.
        settings.enable_read_replica = true;
        settings.replica_url = None;
        settings.replica_params.host = Some("replica-host".into());
        let configs = DatabaseConfigs::from_settings(&settings).unwrap();
        assert_eq!(
            configs.replica.as_ref().unwrap().host.as_deref(),
            Some("replica-host")
        );
    }
}
