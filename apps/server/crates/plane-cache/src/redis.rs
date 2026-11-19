// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! One lazy Redis handle per process, built from `Settings` exactly as
//! django-redis builds its connection pool.
//!
//! `REDIS_URL` is parsed with redis-py's rules (`redis/connection.py:882-936`),
//! not redis-rs' rules, because that is what Django does:
//!
//! * only `redis://`, `rediss://` and `unix://` are accepted;
//! * the DB number comes from `?db=`, else from the path, else 0 — and a bare
//!   trailing slash (`redis://plane-redis:6379/`, the value in `.env.example`)
//!   is *not* an error, because redis-py's `int("")` fails and is swallowed;
//! * username and password are percent-decoded;
//! * query arguments that redis-rs does not understand (everything except
//!   `protocol`) are ignored by redis-py or consumed by us (`db`,
//!   `ssl_cert_reqs`, `ssl_check_hostname`).
//!
//! TLS follows `_redis_ssl_kwargs` (`plane/settings/common.py:253-271`):
//! `ssl_cert_reqs` (URL query first, then `REDIS_SSL_CERT_REQS`) decides
//! verification, `ssl_ca_certs` replaces the system roots, and
//! `ssl_check_hostname` defaults to `cert_reqs != "none"`.
//!
//! The connection is *lazy*: `Client::open` only validates the URL, and the
//! `ConnectionManager` (which connects eagerly and reconnects in the
//! background) is created on the first command. The handle is cheap to clone
//! and shares one connection manager, which is what "shared per process"
//! means here — construct it once in the application state.

use std::sync::Arc;
use std::time::Duration;

use redis::aio::ConnectionManager;
use redis::{Client, FromRedisValue, TlsCertificates};
use tokio::sync::OnceCell;

use crate::lua::EvalArg;

use plane_core::Settings;
use plane_core::config::RedisSettings;
use plane_core::py;

use crate::error::Error;

/// How long a command may wait for a reply. django-redis leaves the redis-py
/// default (`socket_timeout=None`, no timeout) but a hung Redis must not pin a
/// request worker forever; 5 s is far above the p99 of a `GET`/`SET` and is
/// still an error rather than a hang.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Connection establishment timeout (redis-py: `socket_connect_timeout=None`,
/// i.e. the OS default). Kept finite for the same reason.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// A cloneable handle to the process-wide Redis connection.
#[derive(Debug, Clone)]
pub struct Redis {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    client: Client,
    manager: OnceCell<ConnectionManager>,
    /// `RedisConnectionInfo::db` of the client, for logging and tests.
    db: i64,
    /// `host:port/db` with credentials removed.
    display: String,
}

impl Redis {
    /// Build the handle from `Settings` (`REDIS_URL`, `REDIS_SSL_*`).
    pub fn from_settings(settings: &Settings) -> Result<Self, Error> {
        Self::from_redis_settings(&settings.redis)
    }

    /// Build the handle from the Redis settings alone.
    pub fn from_redis_settings(settings: &RedisSettings) -> Result<Self, Error> {
        let url = settings.url.as_deref().ok_or(Error::RedisUrlMissing)?;
        Self::from_url_with_ssl(url, settings)
    }

    /// Build the handle from a bare URL with no SSL overrides.
    ///
    /// Equivalent to what `_redis_ssl_kwargs` derives from the URL itself:
    /// `ssl_cert_reqs` from the query string (default `required`) and no CA
    /// file. `REDIS_SSL_CERT_REQS` and `REDIS_SSL_CA_CERTS` are only honoured
    /// through [`Redis::from_settings`], mirroring the fact that Plane's
    /// settings module is the only place Django reads them.
    pub fn from_url(url: &str) -> Result<Self, Error> {
        let parsed = ParsedUrl::parse(url)?;
        validate_tls(&parsed)?;
        let client = ClientBuilder {
            parsed: &parsed,
            ca_certs: None,
        }
        .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                client,
                manager: OnceCell::new(),
                db: parsed.db,
                display: parsed.display(),
            }),
        })
    }

    fn from_url_with_ssl(url: &str, settings: &RedisSettings) -> Result<Self, Error> {
        let parsed = ParsedUrl::parse_with_cert_reqs(url, Some(&settings.ssl_cert_reqs))?;
        let client = ClientBuilder::new(&parsed, settings)?.build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                client,
                manager: OnceCell::new(),
                db: parsed.db,
                display: parsed.display(),
            }),
        })
    }

    /// The `db` index this handle points at.
    pub fn db(&self) -> i64 {
        self.inner.db
    }

    /// `host:port/db`, never a password. Safe to log.
    pub fn display(&self) -> &str {
        &self.inner.display
    }

    /// The shared connection manager, connecting on first use.
    pub async fn connection(&self) -> Result<ConnectionManager, Error> {
        let manager = self
            .inner
            .manager
            .get_or_try_init(|| async {
                let config = redis::aio::ConnectionManagerConfig::new()
                    .set_connection_timeout(Some(CONNECTION_TIMEOUT))
                    .set_response_timeout(Some(RESPONSE_TIMEOUT));
                ConnectionManager::new_with_config(self.inner.client.clone(), config).await
            })
            .await?;
        Ok(manager.clone())
    }

    /// A raw `GET`.
    pub async fn get_bytes(&self, full_key: &str) -> Result<Option<Vec<u8>>, Error> {
        let mut connection = self.connection().await?;
        let value: Option<Vec<u8>> = redis::cmd("GET")
            .arg(full_key)
            .query_async(&mut connection)
            .await?;
        Ok(value)
    }

    /// `MGET`, the command `DefaultClient.get_many` issues.
    pub async fn mget(&self, full_keys: &[String]) -> Result<Vec<Option<Vec<u8>>>, Error> {
        if full_keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut connection = self.connection().await?;
        let mut command = redis::cmd("MGET");
        for key in full_keys {
            command.arg(key);
        }
        let values: Vec<Option<Vec<u8>>> = command.query_async(&mut connection).await?;
        Ok(values)
    }

    /// `SET key value`, the command `DefaultClient.set` issues without a
    /// timeout.
    pub async fn set_bytes(&self, full_key: &str, value: &[u8]) -> Result<(), Error> {
        let mut connection = self.connection().await?;
        let _: () = redis::cmd("SET")
            .arg(full_key)
            .arg(value)
            .query_async(&mut connection)
            .await?;
        Ok(())
    }

    /// `SET key value EX <seconds>`, the command redis-py issues for
    /// `ri.set(key, value, ex=600)` (`plane/bgtasks/issue_activities_task.py:1577`).
    pub async fn set_bytes_ex(
        &self,
        full_key: &str,
        value: &[u8],
        seconds: i64,
    ) -> Result<(), Error> {
        let mut connection = self.connection().await?;
        let _: () = redis::cmd("SET")
            .arg(full_key)
            .arg(value)
            .arg("EX")
            .arg(seconds)
            .query_async(&mut connection)
            .await?;
        Ok(())
    }

    /// `SET key value PX <milliseconds>`, the command `DefaultClient.set`
    /// issues with a timeout (`django_redis/client/default.py:257-283`).
    pub async fn set_bytes_px(
        &self,
        full_key: &str,
        value: &[u8],
        milliseconds: i64,
    ) -> Result<(), Error> {
        let mut connection = self.connection().await?;
        let _: () = redis::cmd("SET")
            .arg(full_key)
            .arg(value)
            .arg("PX")
            .arg(milliseconds)
            .query_async(&mut connection)
            .await?;
        Ok(())
    }

    /// `redis-py`'s `Redis.eval(script, numkeys, *keys_and_args)`
    /// (`redis/client.py:2707-2780`).
    ///
    /// `numkeys` is `keys.len()`; `args` follow the keys, in order, and are
    /// encoded the way redis-py encodes a command argument (an `int` becomes
    /// its decimal text, a `str` its UTF-8 bytes). The reply is decoded into
    /// `T` with [`FromRedisValue`], so a script that returns a different shape
    /// is an error — a `ResponseError` in Python, a 500 in a request.
    ///
    /// redis-py has no parameter binding: the script and its arguments all
    /// travel as `bulk` strings inside one `EVAL` command. This method issues
    /// exactly that command, so an existing script keeps its SHA and the
    /// server-side effect order is unchanged.
    pub async fn eval<T: FromRedisValue>(
        &self,
        script: &str,
        keys: &[&str],
        args: &[EvalArg<'_>],
    ) -> Result<T, Error> {
        let mut connection = self.connection().await?;
        let mut command = redis::cmd("EVAL");
        command.arg(script).arg(keys.len() as i64);
        for key in keys {
            command.arg(*key);
        }
        for arg in args {
            match arg {
                EvalArg::Int(value) => {
                    command.arg(*value);
                }
                EvalArg::Str(value) => {
                    command.arg(*value);
                }
                EvalArg::Bytes(value) => {
                    command.arg(*value);
                }
            }
        }
        Ok(command.query_async(&mut connection).await?)
    }

    /// Atomic `INCR`, with `EXPIRE` **only on the first increment**
    /// (`plane/authentication/provider/credentials/magic_code.py:40-46`).
    ///
    /// `INCR` preserves an existing TTL, so every later increment keeps the
    /// window that the first one set — which is what makes the counter expire
    /// with the key it counts. Returns the new counter value.
    pub async fn incr_expire_on_first(
        &self,
        full_key: &str,
        ttl_seconds: i64,
    ) -> Result<i64, Error> {
        self.eval(
            crate::lua::INCR_EXPIRE_ON_FIRST,
            &[full_key],
            &[EvalArg::Int(ttl_seconds)],
        )
        .await
    }

    /// `SET key value NX`, the command `DefaultClient.set(..., nx=True)`
    /// issues without a timeout (`django_redis/client/default.py:270-283`).
    /// Returns whether the key was written (`SET NX` replies `nil` when the
    /// key already exists).
    pub async fn set_bytes_nx(&self, full_key: &str, value: &[u8]) -> Result<bool, Error> {
        let mut connection = self.connection().await?;
        let reply: Option<String> = redis::cmd("SET")
            .arg(full_key)
            .arg(value)
            .arg("NX")
            .query_async(&mut connection)
            .await?;
        Ok(reply.is_some())
    }

    /// `SET key value NX EX <seconds>`, the command redis-py issues for
    /// `ri.set(key, value, nx=True, ex=300)` (`email_notification_task.py:35-38`).
    /// Returns whether the key was written (`nil` when it already exists).
    pub async fn set_bytes_nx_ex(
        &self,
        full_key: &str,
        value: &[u8],
        seconds: i64,
    ) -> Result<bool, Error> {
        let mut connection = self.connection().await?;
        let reply: Option<String> = redis::cmd("SET")
            .arg(full_key)
            .arg(value)
            .arg("NX")
            .arg("EX")
            .arg(seconds)
            .query_async(&mut connection)
            .await?;
        Ok(reply.is_some())
    }

    /// `SET key value NX PX <milliseconds>`: `cache.add(key, value, timeout)`
    /// (`django_redis/client/default.py:240-283`).
    pub async fn set_bytes_nx_px(
        &self,
        full_key: &str,
        value: &[u8],
        milliseconds: i64,
    ) -> Result<bool, Error> {
        let mut connection = self.connection().await?;
        let reply: Option<String> = redis::cmd("SET")
            .arg(full_key)
            .arg(value)
            .arg("NX")
            .arg("PX")
            .arg(milliseconds)
            .query_async(&mut connection)
            .await?;
        Ok(reply.is_some())
    }

    /// `EXISTS key`, used by `add`'s non-positive-timeout branch and by
    /// `incr`'s error path (`django_redis/client/default.py:257-268`).
    pub async fn exists(&self, full_key: &str) -> Result<bool, Error> {
        let mut connection = self.connection().await?;
        let count: i64 = redis::cmd("EXISTS")
            .arg(full_key)
            .query_async(&mut connection)
            .await?;
        Ok(count > 0)
    }

    /// `DefaultClient._incr`'s Lua script (`default.py:685-696`): `INCRBY`
    /// only when the key exists, atomically. `None` means the key was gone
    /// (the script returned `false`), which Python turns into a `ValueError`.
    ///
    /// `INCRBY` preserves the key's TTL, which is why django-redis does not
    /// touch it: the window starts at the first `add` and every increment
    /// keeps the same expiry.
    pub async fn incr_by_present(&self, full_key: &str, delta: i64) -> Result<Option<i64>, Error> {
        const SCRIPT: &str = "local exists = redis.call('EXISTS', KEYS[1])\n\
                              if (exists == 1) then\n\
                                return redis.call('INCRBY', KEYS[1], ARGV[1])\n\
                              else return false end";
        self.eval(SCRIPT, &[full_key], &[EvalArg::Int(delta)]).await
    }

    /// `TTL key` in seconds, as django-redis's `ttl()` reads it: `-1` means
    /// no expiry and `-2` a missing key (`default.py:755-780`).
    pub async fn ttl_seconds(&self, full_key: &str) -> Result<i64, Error> {
        let mut connection = self.connection().await?;
        let ttl: i64 = redis::cmd("TTL")
            .arg(full_key)
            .query_async(&mut connection)
            .await?;
        Ok(ttl)
    }

    /// `DEL key`. Returns whether a key was removed.
    pub async fn del(&self, full_key: &str) -> Result<bool, Error> {
        let mut connection = self.connection().await?;
        let deleted: i64 = redis::cmd("DEL")
            .arg(full_key)
            .query_async(&mut connection)
            .await?;
        Ok(deleted > 0)
    }

    /// `DEL key...`. Returns how many keys were removed.
    ///
    /// An empty list issues no command, which is what
    /// `DefaultClient.delete_many` does (`default.py:527-548`).
    pub async fn del_many(&self, full_keys: &[String]) -> Result<usize, Error> {
        if full_keys.is_empty() {
            return Ok(0);
        }
        let mut connection = self.connection().await?;
        let mut command = redis::cmd("DEL");
        for key in full_keys {
            command.arg(key);
        }
        let deleted: i64 = command.query_async(&mut connection).await?;
        Ok(usize::try_from(deleted).unwrap_or(0))
    }

    /// `KEYS <pattern>`, the command `DefaultClient.keys` issues.
    ///
    /// `KEYS` walks the whole keyspace and blocks the server for the duration,
    /// which is why `delete_pattern`/`iter_keys` exist. `plane/utils/cache.py`
    /// calls `cache.keys(f"*{key}*")` anyway, so the port issues the same
    /// command instead of silently switching to `SCAN`: the server-side cost,
    /// the latency spike and the blocking behaviour are part of the behaviour
    /// being reproduced (`django_redis/client/default.py:851-874`).
    pub async fn keys(&self, pattern: &str) -> Result<Vec<String>, Error> {
        let mut connection = self.connection().await?;
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query_async(&mut connection)
            .await?;
        Ok(keys)
    }

    /// `PTTL` of a full key in milliseconds (`-1` no TTL, `-2` missing).
    pub async fn pttl_ms(&self, full_key: &str) -> Result<i64, Error> {
        let mut connection = self.connection().await?;
        let ttl: i64 = redis::cmd("PTTL")
            .arg(full_key)
            .query_async(&mut connection)
            .await?;
        Ok(ttl)
    }

    /// `FLUSHDB` on the selected database: `django_redis`'s
    /// `DefaultClient.clear()` (`django_redis/client/default.py:550-561`).
    ///
    /// Production caller: `plane-bootstrap`'s `clear_cache` boot step, which
    /// ports `manage.py clear_cache` as-is (it wipes throttle buckets and
    /// magic-link codes on every API start). Tests must still verify the URL
    /// points at one of the isolated dev databases (`ENV.md`, `AGENTS.md`
    /// "Isolation between concurrent agents").
    pub async fn flush_db(&self) -> Result<(), Error> {
        let mut connection = self.connection().await?;
        let _: () = redis::cmd("FLUSHDB").query_async(&mut connection).await?;
        Ok(())
    }
}

/// The parts of `REDIS_URL` that matter once redis-py has parsed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    /// `redis`, `rediss` or `unix`.
    pub scheme: String,
    /// `host:port`, or the socket path for `unix`.
    pub host_port: String,
    /// Decoded username, if any.
    pub username: Option<String>,
    /// Decoded password, if any.
    pub password: Option<String>,
    /// Database index (query `db` first, then the path, then 0).
    pub db: i64,
    /// TLS options that apply to this URL.
    pub cert_reqs: String,
    /// `ssl_check_hostname`, after the URL query and the `none` default.
    pub check_hostname: bool,
}

impl ParsedUrl {
    /// redis-py's `parse_url` (`redis/connection.py:882-936`) plus
    /// `_redis_ssl_kwargs` (`plane/settings/common.py:253-271`).
    pub fn parse(url: &str) -> Result<Self, Error> {
        Self::parse_with_cert_reqs(url, None)
    }

    fn parse_with_cert_reqs(url: &str, env_cert_reqs: Option<&str>) -> Result<Self, Error> {
        let scheme = url.split("://").next().unwrap_or("");
        if !matches!(scheme, "redis" | "rediss" | "unix") || !url.contains("://") {
            return Err(Error::RedisUrlScheme);
        }

        let split = py::urlsplit(url).map_err(|_| Error::RedisUrl("could not parse URL"))?;
        let query = py::parse_qs(&split.query);
        let query_value = |name: &str| {
            query
                .iter()
                .find(|(key, _)| key == name)
                .and_then(|(_, values)| values.first())
                .cloned()
        };

        // `db`: `?db=` wins over the path (`parse_url` returns before the
        // path branch is reached), the path is `int(path.replace("/", ""))`
        // and a failure there is swallowed.
        let mut db = 0_i64;
        if let Some(value) = query_value("db") {
            db = py::int(&value).map_err(|_| Error::RedisUrl("invalid db in query"))?;
        } else if !split.path.is_empty() {
            let digits = py::unquote(&split.path).replace('/', "");
            if let Ok(value) = py::int(&digits) {
                db = value;
            }
        }

        let (username, password) = split_userinfo(&split.netloc);

        let (host_port, scheme) = if scheme == "unix" {
            (py::unquote(&split.path), scheme.to_string())
        } else {
            let host_port = split
                .netloc
                .rsplit_once('@')
                .map(|(_, host)| host)
                .unwrap_or(&split.netloc)
                .to_string();
            let host_port = if host_port.is_empty() {
                "localhost:6379".to_string()
            } else {
                host_port
            };
            // Accessing `url.port` raises `ValueError` for a bad port in
            // Python; surface the same before connecting.
            split
                .port()
                .map_err(|_| Error::RedisUrl("port could not be cast to integer"))?;
            (host_port, scheme.to_string())
        };

        let url_cert_reqs = query_value("ssl_cert_reqs");
        let cert_reqs = (url_cert_reqs.as_deref().or(env_cert_reqs).unwrap_or(""))
            .trim()
            .to_lowercase();
        let cert_reqs = if cert_reqs.is_empty() {
            "required".to_string()
        } else {
            cert_reqs
        };
        // The value is only validated by `SSLConnection`, i.e. when a `rediss`
        // pool is actually built (`plane/settings/common.py:273-291` passes
        // the kwargs only when `REDIS_SSL` is truthy).
        let check_hostname = match query_value("ssl_check_hostname") {
            Some(value) => to_bool(&value),
            None => cert_reqs != "none",
        };

        Ok(Self {
            scheme,
            host_port,
            username,
            password,
            db,
            cert_reqs,
            check_hostname,
        })
    }

    /// `rediss://host:port/db`, credentials included. Only handed to
    /// `redis::Client`, never logged.
    fn client_url(&self) -> String {
        let mut url = String::with_capacity(self.host_port.len() + 32);
        url.push_str(&self.scheme);
        url.push_str("://");
        if self.username.is_some() || self.password.is_some() {
            url.push_str(&percent_encode(self.username.as_deref().unwrap_or("")));
            if let Some(password) = &self.password {
                url.push(':');
                url.push_str(&percent_encode(password));
            }
            url.push('@');
        }
        url.push_str(&self.host_port);
        if self.scheme == "unix" {
            // `unix://<path>?db=<n>`; redis-rs reads `db` from the query for
            // unix sockets, matching redis-py.
            url.push_str(&format!("?db={}", self.db));
        } else {
            url.push('/');
            url.push_str(&self.db.to_string());
        }
        url
    }

    /// `host:port/db` with no credentials.
    fn display(&self) -> String {
        if self.scheme == "unix" {
            format!("unix:{}", self.host_port)
        } else {
            format!("redis:{}:{}", self.host_port, self.db)
        }
    }

    /// True when the URL scheme asks for TLS.
    fn tls(&self) -> bool {
        self.scheme == "rediss"
    }
}

/// Builds a `redis::Client`, applying the TLS options.
struct ClientBuilder<'a> {
    parsed: &'a ParsedUrl,
    ca_certs: Option<&'a str>,
}

impl<'a> ClientBuilder<'a> {
    fn new(parsed: &'a ParsedUrl, settings: &'a RedisSettings) -> Result<Self, Error> {
        validate_tls(parsed)?;
        Ok(Self {
            parsed,
            ca_certs: settings.ssl_ca_certs.as_deref(),
        })
    }

    fn build(&self) -> Result<Client, Error> {
        let url = self.parsed.client_url();
        if !self.parsed.tls() {
            return Client::open(url).map_err(Error::Redis);
        }
        // `#insecure` is redis-rs' spelling of `ssl_cert_reqs=none`
        // (`connection.rs:620-645`); the feature it needs
        // (`tls-rustls-insecure`) is enabled in Cargo.toml.
        let url = if self.parsed.cert_reqs == "none" {
            format!("{url}#insecure")
        } else {
            url
        };
        match self.ca_certs {
            None => Client::open(url).map_err(Error::Redis),
            Some(path) => {
                let pem = std::fs::read(path)
                    .map_err(|error| Error::SslCaCerts(format!("{path}: {error}")))?;
                Client::build_with_tls(
                    url,
                    TlsCertificates {
                        client_tls: None,
                        root_cert: Some(pem),
                    },
                )
                .map_err(Error::Redis)
            }
        }
    }
}

/// Checks that apply only to a TLS URL: `SSLConnection` is the only thing that
/// ever looks at `ssl_cert_reqs`, so a `redis://` deployment with a nonsense
/// value keeps working (as it does in Python).
fn validate_tls(parsed: &ParsedUrl) -> Result<(), Error> {
    if !parsed.tls() {
        return Ok(());
    }
    if !matches!(parsed.cert_reqs.as_str(), "none" | "optional" | "required") {
        return Err(Error::SslCertReqs(parsed.cert_reqs.clone()));
    }
    if !parsed.check_hostname && parsed.cert_reqs != "none" {
        // redis-py can verify the chain without matching the hostname; rustls
        // (through redis-rs) cannot. Keep verification on, which is the safe
        // direction, and say so.
        tracing::warn!(
            target: "plane",
            "REDIS_URL sets ssl_check_hostname=false; the Rust client keeps hostname \
             verification enabled because rustls cannot separate the two checks"
        );
    }
    Ok(())
}

fn split_userinfo(netloc: &str) -> (Option<String>, Option<String>) {
    let Some((userinfo, _)) = netloc.rsplit_once('@') else {
        return (None, None);
    };
    match userinfo.split_once(':') {
        Some((user, password)) => (
            if user.is_empty() {
                None
            } else {
                Some(py::unquote(user))
            },
            if password.is_empty() {
                None
            } else {
                Some(py::unquote(password))
            },
        ),
        None => (
            if userinfo.is_empty() {
                None
            } else {
                Some(py::unquote(userinfo))
            },
            None,
        ),
    }
}

/// redis-py's `to_bool` (`redis/utils.py`): case-insensitive `1/true/yes/on`.
fn to_bool(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Percent-encode a URL userinfo component, keeping the sub-delims redis-py
/// leaves alone.
fn percent_encode(value: &str) -> String {
    const UNRESERVED: &[u8] = b"-._~!$&'()*+,;=";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || UNRESERVED.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plane_urls() {
        // The `.env.example` value: trailing slash, no database.
        let parsed = ParsedUrl::parse("redis://plane-redis:6379/").unwrap();
        assert_eq!(parsed.db, 0);
        assert_eq!(parsed.host_port, "plane-redis:6379");
        assert_eq!(parsed.client_url(), "redis://plane-redis:6379/0");
        assert!(!parsed.tls());

        let parsed = ParsedUrl::parse("redis://127.0.0.1:56379/13").unwrap();
        assert_eq!(parsed.db, 13);
        assert_eq!(parsed.client_url(), "redis://127.0.0.1:56379/13");

        // `?db=` overrides the path (redis-py: "querystring value wins").
        let parsed = ParsedUrl::parse("redis://host:6379/9?db=3").unwrap();
        assert_eq!(parsed.db, 3);

        // A path that is not a number is ignored, not an error.
        let parsed = ParsedUrl::parse("redis://host:6379/notadb").unwrap();
        assert_eq!(parsed.db, 0);
    }

    #[test]
    fn parses_credentials_and_tls() {
        let parsed =
            ParsedUrl::parse("rediss://user:p%40ss%3Aword@redis.example.com:6380/2").unwrap();
        assert_eq!(parsed.username.as_deref(), Some("user"));
        assert_eq!(parsed.password.as_deref(), Some("p@ss:word"));
        assert_eq!(parsed.db, 2);
        assert!(parsed.tls());
        assert_eq!(parsed.cert_reqs, "required");
        assert!(parsed.check_hostname);
        assert_eq!(
            parsed.client_url(),
            "rediss://user:p%40ss%3Aword@redis.example.com:6380/2"
        );

        let parsed = ParsedUrl::parse("redis://:secret@host:6379/0").unwrap();
        assert_eq!(parsed.username, None);
        assert_eq!(parsed.password.as_deref(), Some("secret"));
        assert_eq!(parsed.client_url(), "redis://:secret@host:6379/0");
    }

    #[test]
    fn ssl_options_follow_plane_settings() {
        // ENV.md §1.4: the URL query wins over the environment.
        let parsed =
            ParsedUrl::parse_with_cert_reqs("rediss://host:6379/0", Some("required")).unwrap();
        assert_eq!(parsed.cert_reqs, "required");
        assert!(parsed.check_hostname);

        let parsed = ParsedUrl::parse_with_cert_reqs(
            "rediss://host:6379/0?ssl_cert_reqs=none",
            Some("required"),
        )
        .unwrap();
        assert_eq!(parsed.cert_reqs, "none");
        assert!(!parsed.check_hostname);

        let parsed = ParsedUrl::parse_with_cert_reqs(
            "rediss://host:6379/0?ssl_check_hostname=false",
            Some("required"),
        )
        .unwrap();
        assert!(!parsed.check_hostname);
        assert_eq!(parsed.cert_reqs, "required");

        // `(value or env).strip().lower() or "required"`.
        let parsed = ParsedUrl::parse_with_cert_reqs("rediss://host:6379/0", Some("  ")).unwrap();
        assert_eq!(parsed.cert_reqs, "required");

        let parsed =
            ParsedUrl::parse_with_cert_reqs("rediss://host/0", Some("cert_required")).unwrap();
        assert!(matches!(
            validate_tls(&parsed),
            Err(Error::SslCertReqs(value)) if value == "cert_required"
        ));
        // ... but only for a TLS URL: `redis://` never builds an SSLConnection.
        let parsed =
            ParsedUrl::parse_with_cert_reqs("redis://host/0", Some("cert_required")).unwrap();
        assert!(validate_tls(&parsed).is_ok());
    }

    #[test]
    fn rejects_other_schemes_like_redis_py() {
        assert!(matches!(
            ParsedUrl::parse("http://localhost:6379/0"),
            Err(Error::RedisUrlScheme)
        ));
        assert!(matches!(
            ParsedUrl::parse("localhost:6379"),
            Err(Error::RedisUrlScheme)
        ));
    }

    #[test]
    fn rejects_bad_ports_like_urlparse() {
        assert!(matches!(
            ParsedUrl::parse("redis://localhost:notaport/0"),
            Err(Error::RedisUrl(_))
        ));
    }

    #[test]
    fn builds_a_client_without_connecting() {
        // `Client::open` performs no I/O; this must not need a live server.
        let redis = Redis::from_url("redis://127.0.0.1:56379/9").unwrap();
        assert_eq!(redis.db(), 9);
        assert_eq!(redis.display(), "redis:127.0.0.1:56379:9");
    }

    #[test]
    fn booleans_match_redis_py() {
        assert!(to_bool("1"));
        assert!(to_bool("TRUE"));
        assert!(to_bool("yes"));
        assert!(to_bool("On"));
        assert!(!to_bool("0"));
        assert!(!to_bool("false"));
        assert!(!to_bool(""));
    }
}
