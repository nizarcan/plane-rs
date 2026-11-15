// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Connection pools and the read/write routing rule.
//!
//! Connections are lazy: `deadpool`'s `Pool::builder(..).build()` does not
//! open anything, so `plane-api` starts with Postgres down and `GET /` (the
//! health check) never touches the database. That matches Django, whose
//! `django.db.connections` objects are created at import but connect on first
//! cursor.
//!
//! Routing reproduces `plane/middleware/db_routing.py` plus
//! `plane/utils/core/dbrouters.py`: writes always go to the primary; a read
//! (`GET`/`HEAD`/`OPTIONS`) goes to the replica only when the view opted in
//! with `use_read_replica = True` and a replica is configured. With no replica
//! configured the router is not installed at all, so everything is primary.

use std::collections::BTreeMap;
use std::time::Duration;

use deadpool_postgres::{Manager, Pool, PoolError};
use http::Method;
use plane_core::config::Settings;
use tokio_postgres::config::ChannelBinding;

use crate::tls::{self, SslMode, TlsError};
use crate::url::{DatabaseConfig, DatabaseConfigs, DatabaseUrlError, OptionValue};

/// Anything that can go wrong while building or using the pools.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Url(#[from] DatabaseUrlError),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(
        "database NAME is empty: supply a name in DATABASE_URL or POSTGRES_DB (Django raises ImproperlyConfigured)"
    )]
    EmptyName,
    #[error("database NAME is longer than PostgreSQL's 63-character limit")]
    NameTooLong,
    #[error("OPTIONS.{0} must be a scalar, not a list")]
    ListOption(String),
    #[error("invalid value for OPTIONS.{name}: {value}")]
    InvalidOption { name: &'static str, value: String },
    #[error("invalid port {0:?}")]
    InvalidPort(String),
    #[error("cannot build the connection pool: {0}")]
    PoolBuild(String),
    #[error(transparent)]
    Pool(#[from] PoolError),
    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),
}

/// A fully prepared Postgres connection description.
pub struct PostgresSettings {
    pub config: tokio_postgres::Config,
    pub connector: tokio_postgres_rustls::MakeRustlsConnect,
    pub sslmode: SslMode,
    /// OPTIONS keys psycopg accepts but this port does not act on yet.
    pub unsupported_options: Vec<String>,
}

impl std::fmt::Debug for PostgresSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print credentials.
        f.debug_struct("PostgresSettings")
            .field("sslmode", &self.sslmode)
            .field("unsupported_options", &self.unsupported_options)
            .finish_non_exhaustive()
    }
}

impl PostgresSettings {
    /// Port of Django's `DatabaseWrapper.get_connection_params`
    /// (`django/db/backends/postgresql/base.py:237-292`) plus the `sslmode`
    /// translation.
    pub fn build(config: &DatabaseConfig) -> Result<Self, DbError> {
        let mut pg = tokio_postgres::Config::new();

        // NAME: `""` raises ImproperlyConfigured (unless OPTIONS["service"],
        // which this port does not implement); `None` means the "postgres"
        // database; names are limited to 63 characters.
        match config.name.as_deref() {
            Some("") => {
                if !config.options.contains_key("service") {
                    return Err(DbError::EmptyName);
                }
                pg.dbname("postgres");
            }
            Some(name) => {
                if name.chars().count() > 63 {
                    return Err(DbError::NameTooLong);
                }
                pg.dbname(name);
            }
            None => {
                pg.dbname("postgres");
            }
        }

        // OPTIONS first; the explicit USER/PASSWORD/HOST/PORT overwrite them,
        // exactly like Django's dict construction.
        let mut unsupported_options = Vec::new();
        let mut sslmode = SslMode::DEFAULT;
        let mut sslrootcert: Option<String> = None;
        for (key, value) in &config.options {
            match key.as_str() {
                "sslmode" => {
                    let raw = scalar(key, value)?;
                    sslmode = SslMode::parse(&raw).ok_or_else(|| DbError::InvalidOption {
                        name: "sslmode",
                        value: raw,
                    })?;
                }
                "sslrootcert" => sslrootcert = Some(scalar(key, value)?),
                "options" => {
                    pg.options(scalar(key, value)?);
                }
                "application_name" => {
                    pg.application_name(scalar(key, value)?);
                }
                "connect_timeout" => {
                    let raw = scalar(key, value)?;
                    let seconds: u64 = raw.parse().map_err(|_| DbError::InvalidOption {
                        name: "connect_timeout",
                        value: raw.clone(),
                    })?;
                    pg.connect_timeout(Duration::from_secs(seconds));
                }
                "channel_binding" => {
                    let raw = scalar(key, value)?;
                    pg.channel_binding(match raw.as_str() {
                        "disable" => ChannelBinding::Disable,
                        "prefer" => ChannelBinding::Prefer,
                        "require" => ChannelBinding::Require,
                        _ => {
                            return Err(DbError::InvalidOption {
                                name: "channel_binding",
                                value: raw,
                            });
                        }
                    });
                }
                "hostaddr" => {
                    let raw = scalar(key, value)?;
                    let address: std::net::IpAddr =
                        raw.parse().map_err(|_| DbError::InvalidOption {
                            name: "hostaddr",
                            value: raw.clone(),
                        })?;
                    pg.hostaddr(address);
                }
                // psycopg-only keywords that cannot be expressed in a
                // tokio-postgres `Config`; recorded so the binary can log them.
                other => unsupported_options.push(other.to_string()),
            }
        }

        // USER/PASSWORD/HOST/PORT: `if settings_dict["USER"]:` — empty is
        // skipped, so an empty URL user falls back to libpq's default.
        if let Some(user) = config.user.as_deref().filter(|user| !user.is_empty()) {
            pg.user(user);
        }
        if let Some(password) = config
            .password
            .as_deref()
            .filter(|password| !password.is_empty())
        {
            pg.password(password);
        }
        if let Some(host) = config.host.as_deref().filter(|host| !host.is_empty()) {
            pg.host(host);
        }
        if !config.port.is_empty() {
            let port: u16 = config
                .port
                .parse()
                .map_err(|_| DbError::InvalidPort(config.port.clone()))?;
            pg.port(port);
        }
        pg.ssl_mode(sslmode.as_tokio());

        let connector = tls::connector(sslmode, sslrootcert.as_deref())?;
        Ok(Self {
            config: pg,
            connector,
            sslmode,
            unsupported_options,
        })
    }
}

/// A scalar OPTIONS value as text. `_parse_value` turns all-digit values into
/// integers and `true`/`false` into booleans, but psycopg accepts any of those
/// for its string keywords, so they are rendered back to text here.
fn scalar(key: &str, value: &OptionValue) -> Result<String, DbError> {
    match value {
        OptionValue::Str(value) => Ok(value.clone()),
        OptionValue::Int(value) => Ok(value.to_string()),
        OptionValue::Bool(value) => Ok(value.to_string()),
        OptionValue::List(_) => Err(DbError::ListOption(key.to_string())),
    }
}

/// Primary and optional replica pools.
pub struct DbPools {
    primary: Pool,
    replica: Option<Pool>,
    max_size: usize,
}

impl std::fmt::Debug for DbPools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbPools")
            .field("replica", &self.replica.is_some())
            .field("max_size", &self.max_size)
            .finish()
    }
}

impl DbPools {
    /// Build both pools from `Settings`. No connection is opened.
    pub fn from_settings(settings: &Settings) -> Result<Self, DbError> {
        let configs = DatabaseConfigs::from_settings(&settings.database)?;
        let max_size = settings.rust.db_pool_size;
        let primary = pool(&configs.primary, max_size, "primary")?;
        let replica = match &configs.replica {
            Some(config) => Some(pool(config, max_size, "replica")?),
            None => None,
        };
        Ok(Self {
            primary,
            replica,
            max_size,
        })
    }

    /// The pool Django's router calls "default".
    pub fn primary(&self) -> &Pool {
        &self.primary
    }

    /// The replica, when `ENABLE_READ_REPLICA=1` configured one.
    pub fn replica(&self) -> Option<&Pool> {
        self.replica.as_ref()
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// `ReadReplicaRoutingMiddleware` + `ReadReplicaRouter.db_for_read`:
    /// GET/HEAD/OPTIONS on an opted-in view read from the replica.
    pub fn read_pool(&self, method: &Method, use_read_replica: bool) -> &Pool {
        let is_read = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
        match (is_read && use_read_replica, &self.replica) {
            (true, Some(replica)) => replica,
            _ => &self.primary,
        }
    }

    /// The pool a write (or any non-opted-in statement) uses.
    pub fn write_pool(&self) -> &Pool {
        &self.primary
    }
}

fn pool(config: &DatabaseConfig, max_size: usize, label: &'static str) -> Result<Pool, DbError> {
    let settings = PostgresSettings::build(config)?;
    if !settings.unsupported_options.is_empty() {
        tracing::warn!(
            target: "plane.db",
            pool = label,
            options = ?settings.unsupported_options,
            "database OPTIONS accepted but not applied by plane-db"
        );
    }
    tracing::debug!(
        target: "plane.db",
        pool = label,
        max_size,
        sslmode = ?settings.sslmode,
        "database pool configured (connections are lazy)"
    );
    let manager = Manager::new(settings.config, settings.connector);
    Pool::builder(manager)
        .max_size(max_size)
        .build()
        .map_err(|error| DbError::PoolBuild(error.to_string()))
}

/// Convenience for the common "one short read" shape: check out a connection
/// and prepare the statement through the per-connection statement cache
/// (`DECISIONS.md` D1).
pub async fn prepare_cached(
    pool: &Pool,
    sql: &str,
) -> Result<(deadpool_postgres::Client, tokio_postgres::Statement), DbError> {
    let client = pool.get().await?;
    let statement = client.prepare_cached(sql).await?;
    Ok((client, statement))
}

/// `{"key": 1, "flag": true}` view of OPTIONS, for logging and tests.
pub fn options_summary(options: &BTreeMap<String, OptionValue>) -> String {
    let mut pairs: Vec<String> = Vec::new();
    for (key, value) in options {
        let rendered = match value {
            OptionValue::Int(value) => value.to_string(),
            OptionValue::Bool(value) => value.to_string(),
            OptionValue::Str(value) => format!("{value:?}"),
            OptionValue::List(values) => format!("{values:?}"),
        };
        pairs.push(format!("{key}={rendered}"));
    }
    pairs.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::url::DatabaseConfig;

    fn config(url: &str) -> DatabaseConfig {
        DatabaseConfig::parse(url).expect("url")
    }

    fn settings_with_params(params: plane_core::config::PostgresParams) -> DatabaseConfig {
        DatabaseConfig::from_params(&params)
    }

    #[test]
    fn name_rules() {
        // ENV.md §1.3 + Django base.py:240-264.
        let empty = config("postgres://h:5432");
        assert_eq!(empty.name.as_deref(), Some(""));
        assert!(matches!(
            PostgresSettings::build(&empty).unwrap_err(),
            DbError::EmptyName
        ));

        let long = config(&format!("postgres://h/{}", "a".repeat(64)));
        assert!(matches!(
            PostgresSettings::build(&long).unwrap_err(),
            DbError::NameTooLong
        ));

        let unset = settings_with_params(plane_core::config::PostgresParams::default());
        assert_eq!(unset.name, None);
        let built = PostgresSettings::build(&unset).unwrap();
        assert_eq!(built.config.get_dbname(), Some("postgres"));
    }

    #[test]
    fn user_password_host_port() {
        let built = PostgresSettings::build(&config(
            "postgres://user:pass@example.com:6543/db?sslmode=disable",
        ))
        .unwrap();
        assert_eq!(built.config.get_user(), Some("user"));
        assert_eq!(built.config.get_password(), Some(&b"pass"[..]));
        assert_eq!(built.config.get_dbname(), Some("db"));
        assert_eq!(built.config.get_hosts().len(), 1);
        assert_eq!(built.config.get_ports(), &[6543]);
        assert_eq!(built.sslmode, SslMode::Disable);
        assert!(built.unsupported_options.is_empty());
    }

    #[test]
    fn empty_components_are_skipped_like_django() {
        // `if settings_dict["USER"]:` skips an empty value.
        let built = PostgresSettings::build(&config("postgres://h/db")).unwrap();
        assert_eq!(built.config.get_user(), None);
        assert_eq!(built.config.get_password(), None);
        // An empty host is skipped too (libpq then uses its default socket).
        let built = settings_with_params(plane_core::config::PostgresParams {
            db: Some("plane".into()),
            user: Some("".into()),
            password: Some("".into()),
            host: Some("".into()),
            port: "5432".into(),
        });
        let built = PostgresSettings::build(&built).unwrap();
        assert_eq!(built.config.get_hosts().len(), 0);
        assert_eq!(built.config.get_ports(), &[5432]);
    }

    #[test]
    fn options_are_mapped() {
        let built = PostgresSettings::build(&config(
            "postgres://h/db?sslmode=require&application_name=plane-api&connect_timeout=3&options=-c%20search_path%3Dplane&channel_binding=require",
        ))
        .unwrap();
        assert_eq!(built.sslmode, SslMode::Require);
        assert_eq!(built.config.get_application_name(), Some("plane-api"));
        assert_eq!(
            built.config.get_connect_timeout(),
            Some(&Duration::from_secs(3))
        );
        assert_eq!(built.config.get_options(), Some("-c search_path=plane"));
        assert_eq!(built.config.get_channel_binding(), ChannelBinding::Require);
        assert!(built.unsupported_options.is_empty());
    }

    #[test]
    fn unsupported_options_are_reported_not_fatal() {
        let built = PostgresSettings::build(&config(
            "postgres://h/db?sslcert=/tmp/client.pem&sslkey=/tmp/client.key&gssencmode=disable",
        ))
        .unwrap();
        assert_eq!(
            built.unsupported_options,
            vec![
                "gssencmode".to_string(),
                "sslcert".to_string(),
                "sslkey".to_string()
            ]
        );
    }

    #[test]
    fn invalid_options_are_errors() {
        let bad_mode = config("postgres://h/db?sslmode=bogus");
        assert!(matches!(
            PostgresSettings::build(&bad_mode).unwrap_err(),
            DbError::InvalidOption {
                name: "sslmode",
                ..
            }
        ));

        let bad_timeout = config("postgres://h/db?connect_timeout=soon");
        assert!(matches!(
            PostgresSettings::build(&bad_timeout).unwrap_err(),
            DbError::InvalidOption {
                name: "connect_timeout",
                ..
            }
        ));

        let bad_port = settings_with_params(plane_core::config::PostgresParams {
            db: Some("plane".into()),
            port: "not-a-port".into(),
            ..Default::default()
        });
        assert!(matches!(
            PostgresSettings::build(&bad_port).unwrap_err(),
            DbError::InvalidPort(_)
        ));
    }

    // ENV.md §1.3 / `plane/middleware/db_routing.py`: reads use the replica
    // only for GET/HEAD/OPTIONS on an opted-in view; writes never do.
    #[test]
    fn read_routing() {
        use plane_core::config::{EnvMap, Role, Settings};

        let env = EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set(
                "DATABASE_URL",
                "postgresql://plane:plane@127.0.0.1:55432/plane",
            );
        let settings = Settings::from_source(&env, Role::Api).expect("settings");
        let pools = DbPools::from_settings(&settings).expect("pools");
        assert!(pools.replica().is_none());
        let primary = pools.primary() as *const Pool;
        assert_eq!(pools.read_pool(&Method::GET, true) as *const Pool, primary);
        assert_eq!(pools.read_pool(&Method::POST, true) as *const Pool, primary);
        assert_eq!(pools.write_pool() as *const Pool, primary);

        let env = env.set("ENABLE_READ_REPLICA", "1").set(
            "DATABASE_READ_REPLICA_URL",
            "postgresql://plane:plane@127.0.0.1:55432/plane",
        );
        let settings = Settings::from_source(&env, Role::Api).expect("settings");
        let pools = DbPools::from_settings(&settings).expect("pools");
        assert!(pools.replica().is_some());
        let replica = pools.replica().unwrap() as *const Pool;
        let primary = pools.primary() as *const Pool;
        assert_eq!(pools.read_pool(&Method::GET, true) as *const Pool, replica);
        assert_eq!(pools.read_pool(&Method::HEAD, true) as *const Pool, replica);
        assert_eq!(
            pools.read_pool(&Method::OPTIONS, true) as *const Pool,
            replica
        );
        assert_eq!(pools.read_pool(&Method::GET, false) as *const Pool, primary);
        assert_eq!(pools.read_pool(&Method::POST, true) as *const Pool, primary);
        assert_eq!(pools.write_pool() as *const Pool, primary);
    }
}
