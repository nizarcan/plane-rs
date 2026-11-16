// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Celery broker URL, parsed the way kombu parses it.
//!
//! Celery reads `broker_url` from `AMQP_URL` or builds it from `RABBITMQ_*`
//! (`plane/settings/common.py:354-366`, `apps/server/ENV.md` §2.4), then kombu
//! splits it in `kombu/utils/url.py:46-64`:
//!
//! * `urlparse("amqp://…").scheme` names the transport; everything after
//!   `scheme://` is parsed again as an HTTP URL (`urlparse("http://" + rest)`);
//! * the path loses **one** leading `/`, then `unquote()`s (so the default
//!   vhost is written `//`, and `/%2F` is `/`); an empty path means the
//!   default vhost `/`;
//! * user, password and host are `unquote()`d; an absent value falls back to
//!   `guest`/`guest`/`localhost`;
//! * py-amqp's transport rewrites `localhost` to `127.0.0.1`
//!   (`kombu/transport/pyamqp.py:183-184`), which matters on hosts where
//!   `localhost` resolves to `::1` but RabbitMQ listens on IPv4 only.
//!
//! Celery adds `broker_heartbeat = 120` and `broker_connection_timeout = 4`
//! (`celery/app/defaults.py`), passed separately from the URL. A URL query
//! parameter of the same name is a transport option and overrides them; that
//! is reproduced here so a deployment that already tunes `AMQP_URL` keeps its
//! settings.

use std::time::Duration;

use lapin::uri::{AMQPAuthority, AMQPQueryString, AMQPScheme, AMQPUri, AMQPUserInfo};
use percent_encoding::percent_decode_str;

/// Celery's `broker_heartbeat` default, in seconds.
pub const DEFAULT_HEARTBEAT_SECONDS: u16 = 120;
/// Celery's `broker_connection_timeout` default, in seconds.
pub const DEFAULT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(4);

/// A parsed broker URL and the connection knobs Celery would use.
///
/// Deliberately not `Debug`: the URL carries the broker password.
#[derive(Clone)]
pub struct CeleryConfig {
    /// The parsed URI handed to lapin.
    pub uri: AMQPUri,
    /// Negotiated heartbeat interval, in seconds; `0` disables it.
    pub heartbeat: u16,
    /// TCP connect timeout.
    pub connection_timeout: Duration,
}

/// Why a broker URL was refused.
#[derive(Debug, thiserror::Error)]
pub enum BrokerUrlError {
    /// The URL does not use `amqp://` or `amqps://`.
    #[error("the broker URL must use the amqp:// or amqps:// scheme")]
    Scheme,
    /// The URL has no authority (host).
    #[error("the broker URL is missing a host")]
    MissingHost,
    /// The port is not a decimal number in 0-65535.
    #[error("broker URL port {0:?} is not a valid TCP port")]
    InvalidPort(String),
    /// A query parameter is malformed.
    #[error("broker URL query parameter {name:?} is invalid: {value:?}")]
    InvalidQuery { name: String, value: String },
    /// A TLS option this publisher cannot reproduce.
    #[error(
        "broker URL query parameter {0:?} is not supported by the Celery coexistence publisher; \
         use amqps with the system trust store"
    )]
    UnsupportedOption(String),
}

impl CeleryConfig {
    /// Parse a broker URL with Celery's default heartbeat and timeout.
    ///
    /// # Errors
    ///
    /// Returns [`BrokerUrlError`] for a URL kombu would refuse or one whose
    /// transport options this publisher cannot honour.
    pub fn from_url(url: impl Into<String>) -> Result<Self, BrokerUrlError> {
        Self::with_defaults(url, DEFAULT_HEARTBEAT_SECONDS, DEFAULT_CONNECTION_TIMEOUT)
    }

    /// Parse a broker URL with explicit Celery-side defaults.
    ///
    /// # Errors
    ///
    /// Same as [`CeleryConfig::from_url`].
    pub fn with_defaults(
        url: impl Into<String>,
        default_heartbeat: u16,
        default_timeout: Duration,
    ) -> Result<Self, BrokerUrlError> {
        let url = url.into();
        let parts = BrokerUrl::parse(&url)?;

        let heartbeat = match parts.query_value("heartbeat") {
            Some(value) => value
                .parse::<u16>()
                .map_err(|_| BrokerUrlError::InvalidQuery {
                    name: "heartbeat".to_string(),
                    value: value.to_string(),
                })?,
            None => default_heartbeat,
        };
        let connection_timeout = match parts.query_value("connect_timeout") {
            Some(value) => {
                let seconds = value
                    .parse::<f64>()
                    .map_err(|_| BrokerUrlError::InvalidQuery {
                        name: "connect_timeout".to_string(),
                        value: value.to_string(),
                    })?;
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err(BrokerUrlError::InvalidQuery {
                        name: "connect_timeout".to_string(),
                        value: value.to_string(),
                    });
                }
                Duration::from_secs_f64(seconds)
            }
            None => default_timeout,
        };

        for (name, value) in &parts.query {
            match name.as_str() {
                "heartbeat" | "connect_timeout" => {}
                "ssl_cert_reqs" => {
                    let normalised = value.to_ascii_lowercase();
                    if !matches!(normalised.as_str(), "required" | "cert_required" | "2") {
                        return Err(BrokerUrlError::UnsupportedOption(name.clone()));
                    }
                }
                "ssl_check_hostname" => {
                    let normalised = value.to_ascii_lowercase();
                    if !matches!(normalised.as_str(), "true" | "1" | "yes") {
                        return Err(BrokerUrlError::UnsupportedOption(name.clone()));
                    }
                }
                _ => return Err(BrokerUrlError::UnsupportedOption(name.clone())),
            }
        }

        let scheme = match parts.scheme.as_str() {
            "amqp" => AMQPScheme::AMQP,
            "amqps" => AMQPScheme::AMQPS,
            _ => return Err(BrokerUrlError::Scheme),
        };
        let port = match parts.port {
            // py-amqp treats port 0 as "not given" (`port or self.port`).
            Some(0) | None => scheme.default_port(),
            Some(port) => port,
        };
        // py-amqp's transport rewrites localhost (`pyamqp.py:183-184`).
        let host = if parts.host == "localhost" {
            "127.0.0.1".to_string()
        } else {
            parts.host
        };
        let uri = AMQPUri {
            scheme,
            authority: AMQPAuthority {
                userinfo: AMQPUserInfo {
                    username: parts.username,
                    password: parts.password,
                },
                host,
                port,
            },
            vhost: parts.vhost,
            query: AMQPQueryString {
                frame_max: None,
                channel_max: None,
                heartbeat: Some(heartbeat),
                connection_timeout: Some(
                    u64::try_from(connection_timeout.as_millis()).unwrap_or(u64::MAX),
                ),
                auth_mechanism: None,
            },
        };
        Ok(Self {
            uri,
            heartbeat,
            connection_timeout,
        })
    }
}

/// The pieces `kombu/utils/url.url_to_parts` produces, already unquoted.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BrokerUrl {
    scheme: String,
    host: String,
    port: Option<u16>,
    username: String,
    password: String,
    vhost: String,
    query: Vec<(String, String)>,
}

impl BrokerUrl {
    fn parse(url: &str) -> Result<Self, BrokerUrlError> {
        // Python's urlparse trims surrounding C0 controls/spaces and drops
        // tabs and newlines before parsing (`urllib/parse.py:urlsplit`).
        let cleaned = url
            .trim_matches(|c: char| c <= ' ')
            .replace(['\t', '\n', '\r'], "");
        let Some((scheme, after_scheme)) = cleaned.split_once("://") else {
            return Err(BrokerUrlError::Scheme);
        };
        if scheme != "amqp" && scheme != "amqps" {
            return Err(BrokerUrlError::Scheme);
        }
        let authority_end = after_scheme
            .find(['/', '?', '#'])
            .unwrap_or(after_scheme.len());
        let (authority, rest) = after_scheme.split_at(authority_end);
        // `urllib.parse` rejects a fragment for these schemes in practice
        // because kombu strips everything after `?` for the path; a fragment
        // in the authority is not representable.
        if authority.is_empty() {
            return Err(BrokerUrlError::MissingHost);
        }

        let (userinfo, host_port) = match authority.rfind('@') {
            Some(position) => (&authority[..position], &authority[position + 1..]),
            None => ("", authority),
        };
        let (raw_username, raw_password) = match userinfo.split_once(':') {
            Some((user, password)) => (user, Some(password)),
            None => (userinfo, None),
        };
        let username = if raw_username.is_empty() {
            "guest".to_string()
        } else {
            unquote(raw_username)
        };
        let password = match raw_password {
            Some(password) if !password.is_empty() => unquote(password),
            // py-amqp's default password when the URL omits or empties it.
            _ => "guest".to_string(),
        };

        let (host, port) = split_host_port(host_port)?;
        // `url_to_parts` unquotes the hostname; `urlsplit` lower-cases it.
        let host = unquote(&host).to_ascii_lowercase();
        if host.is_empty() {
            return Err(BrokerUrlError::MissingHost);
        }

        let path_end = rest.find(['?', '#']).unwrap_or(rest.len());
        let path = &rest[..path_end];
        let path = path.strip_prefix('/').unwrap_or(path);
        let vhost = unquote(path);
        let vhost = if vhost.is_empty() {
            "/".to_string()
        } else {
            vhost
        };

        let query = match rest.find('?') {
            Some(position) => {
                let query = &rest[position + 1..];
                let query = query.split('#').next().unwrap_or("");
                parse_query(query)
            }
            None => Vec::new(),
        };

        Ok(Self {
            scheme: scheme.to_string(),
            host,
            port,
            username,
            password,
            vhost,
            query,
        })
    }

    fn query_value(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Split `host`, `host:port`, `[v6]` or `[v6]:port`.
fn split_host_port(host_port: &str) -> Result<(String, Option<u16>), BrokerUrlError> {
    if let Some(rest) = host_port.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return Err(BrokerUrlError::MissingHost);
        };
        let host = &rest[..end];
        let after = &rest[end + 1..];
        let port = match after.strip_prefix(':') {
            Some(port) => Some(parse_port(port)?),
            None if after.is_empty() => None,
            None => return Err(BrokerUrlError::InvalidPort(after.to_string())),
        };
        return Ok((host.to_string(), port));
    }
    match host_port.rsplit_once(':') {
        // An empty port (`host:`) is "not given" in `urlsplit`; anything else
        // must be numeric, and a non-numeric value raises `ValueError` there.
        Some((host, "")) => Ok((host.to_string(), None)),
        Some((host, port)) => Ok((host.to_string(), Some(parse_port(port)?))),
        _ => Ok((host_port.to_string(), None)),
    }
}

fn parse_port(port: &str) -> Result<u16, BrokerUrlError> {
    port.parse::<u16>()
        .map_err(|_| BrokerUrlError::InvalidPort(port.to_string()))
}

/// `urllib.parse.unquote`: percent-decode as UTF-8 with replacement.
fn unquote(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

/// `urllib.parse.parse_qsl(query)`: `&`-separated `key=value`, `+` means a
/// space, blank values are dropped (the default `keep_blank_values=False`).
fn parse_query(query: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (key, value) = match part.split_once('=') {
            Some((key, value)) => (key, value),
            None => (part, ""),
        };
        if value.is_empty() {
            continue;
        }
        pairs.push((unquote_plus(key), unquote_plus(value)));
    }
    pairs
}

fn unquote_plus(value: &str) -> String {
    unquote(&value.replace('+', " "))
}
