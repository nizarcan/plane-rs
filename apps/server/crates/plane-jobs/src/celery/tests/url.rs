// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! kombu's broker URL semantics (`kombu/utils/url.py:46-64`,
//! `kombu/connection.py:261-283`).

use std::time::Duration;

use lapin::uri::AMQPScheme;

use crate::celery::broker_url::{BrokerUrlError, CeleryConfig, DEFAULT_CONNECTION_TIMEOUT};

fn parse(url: &str) -> CeleryConfig {
    CeleryConfig::from_url(url).expect("url parses")
}

#[test]
fn composed_default_url() {
    // `plane/settings/common.py:362-366` with every variable unset.
    let config = parse("amqp://guest:guest@localhost:5672//");
    assert_eq!(config.uri.scheme, AMQPScheme::AMQP);
    assert_eq!(config.uri.authority.host, "127.0.0.1");
    assert_eq!(config.uri.authority.port, 5672);
    assert_eq!(config.uri.authority.userinfo.username, "guest");
    assert_eq!(config.uri.authority.userinfo.password, "guest");
    assert_eq!(config.uri.vhost, "/");
    assert_eq!(config.heartbeat, 120);
    assert_eq!(config.connection_timeout, DEFAULT_CONNECTION_TIMEOUT);
    assert_eq!(config.uri.query.heartbeat, Some(120));
    assert_eq!(config.uri.query.connection_timeout, Some(4000));
}

#[test]
fn vhost_forms() {
    assert_eq!(parse("amqp://u:p@mq:1/plane").uri.vhost, "plane");
    // One leading slash is stripped, the rest is the vhost.
    assert_eq!(parse("amqp://u:p@mq:1//").uri.vhost, "/");
    assert_eq!(parse("amqp://u:p@mq:1/%2F").uri.vhost, "/");
    assert_eq!(parse("amqp://u:p@mq:1/").uri.vhost, "/");
    assert_eq!(parse("amqp://u:p@mq:1/v%2Fw").uri.vhost, "v/w");
    assert_eq!(parse("amqp://u:p@mq:1/a%20b").uri.vhost, "a b");
    // An empty path means the default vhost, not a vhost named "".
    assert_eq!(parse("amqp://u:p@mq:1").uri.vhost, "/");
}

#[test]
fn credentials_are_percent_decoded() {
    let config = parse("amqp://us%40er:p%3Aw%2Fd@mq:1/v");
    assert_eq!(config.uri.authority.userinfo.username, "us@er");
    assert_eq!(config.uri.authority.userinfo.password, "p:w/d");
    // The RABBITMQ_* composer does not escape; kombu splits at the last `@`
    // and the first `:`, so these round-trip the same way.
    let config = parse("amqp://plane@x:p:w@mq:55672/plane");
    assert_eq!(config.uri.authority.userinfo.username, "plane@x");
    assert_eq!(config.uri.authority.userinfo.password, "p:w");
}

#[test]
fn missing_credentials_fall_back_to_guest() {
    let config = parse("amqp://mq:1/v");
    assert_eq!(config.uri.authority.userinfo.username, "guest");
    assert_eq!(config.uri.authority.userinfo.password, "guest");
    let config = parse("amqp://u:@mq:1/v");
    assert_eq!(config.uri.authority.userinfo.password, "guest");
}

#[test]
fn amqps_and_ports() {
    let config = parse("amqps://u:p@mq/v");
    assert_eq!(config.uri.scheme, AMQPScheme::AMQPS);
    assert_eq!(config.uri.authority.port, 5671);
    // py-amqp's `port or self.port` turns an explicit 0 into the default.
    assert_eq!(parse("amqp://u:p@mq:0/v").uri.authority.port, 5672);
    assert_eq!(parse("amqps://u:p@mq:0/v").uri.authority.port, 5671);
}

#[test]
fn ipv6_hosts() {
    let config = parse("amqp://u:p@[::1]:5673/v");
    assert_eq!(config.uri.authority.host, "::1");
    assert_eq!(config.uri.authority.port, 5673);
    let config = parse("amqp://u:p@[::1]/v");
    assert_eq!(config.uri.authority.host, "::1");
    assert_eq!(config.uri.authority.port, 5672);
}

#[test]
fn transport_options_override_the_defaults() {
    let config = parse("amqp://u:p@mq/v?heartbeat=60&connect_timeout=1.5");
    assert_eq!(config.heartbeat, 60);
    assert_eq!(config.uri.query.heartbeat, Some(60));
    assert_eq!(config.connection_timeout, Duration::from_millis(1500));
    assert_eq!(config.uri.query.connection_timeout, Some(1500));
    // parse_qsl drops blank values, so `?heartbeat=` keeps the default.
    assert_eq!(parse("amqp://u:p@mq/v?heartbeat=").heartbeat, 120);
    // `+` means a space in a query value, as `unquote_plus` does.
    assert!(matches!(
        CeleryConfig::from_url("amqp://u:p@mq/v?heart+beat=1"),
        Err(BrokerUrlError::UnsupportedOption(_))
    ));
}

#[test]
fn unsupported_options_are_refused() {
    for url in [
        "amqp://u:p@mq/v?ssl_certfile=/tmp/cert.pem",
        "amqp://u:p@mq/v?ssl_ca_certs=/tmp/ca.pem",
        "amqp://u:p@mq/v?ssl_cert_reqs=none",
        "amqp://u:p@mq/v?ssl_check_hostname=false",
        "amqp://u:p@mq/v?unknown=1",
    ] {
        assert!(
            matches!(
                CeleryConfig::from_url(url),
                Err(BrokerUrlError::UnsupportedOption(_))
            ),
            "{url}"
        );
    }
    // The default verification mode is representable.
    parse("amqp://u:p@mq/v?ssl_cert_reqs=required&ssl_check_hostname=true");
}

#[test]
fn invalid_urls() {
    assert!(matches!(
        CeleryConfig::from_url("memory://"),
        Err(BrokerUrlError::Scheme)
    ));
    assert!(matches!(
        CeleryConfig::from_url("mq:5672"),
        Err(BrokerUrlError::Scheme)
    ));
    assert!(matches!(
        CeleryConfig::from_url("amqp://"),
        Err(BrokerUrlError::MissingHost)
    ));
    assert!(matches!(
        CeleryConfig::from_url("amqp://u:p@mq:notaport/v"),
        Err(BrokerUrlError::InvalidPort(_))
    ));
    assert!(matches!(
        CeleryConfig::from_url("amqp://u:p@mq:99999/v"),
        Err(BrokerUrlError::InvalidPort(_))
    ));
    assert!(matches!(
        CeleryConfig::from_url("amqp://u:p@mq/v?heartbeat=soon"),
        Err(BrokerUrlError::InvalidQuery { .. })
    ));
}

#[test]
fn localhost_is_rewritten_like_py_amqp() {
    // `kombu/transport/pyamqp.py:183-184`.
    assert_eq!(
        parse("amqp://u:p@localhost/v").uri.authority.host,
        "127.0.0.1"
    );
    assert_eq!(
        parse("amqp://u:p@127.0.0.1/v").uri.authority.host,
        "127.0.0.1"
    );
    assert_eq!(
        parse("amqp://u:p@mq.local/v").uri.authority.host,
        "mq.local"
    );
    // `urlsplit` lower-cases the hostname; `url_to_parts` unquotes it.
    assert_eq!(
        parse("amqp://u:p@LOCALHOST/v").uri.authority.host,
        "127.0.0.1"
    );
    assert_eq!(
        parse("amqp://u:p@ho%73t.example/v").uri.authority.host,
        "host.example"
    );
}

#[test]
fn an_empty_port_is_the_default_port() {
    // `urlsplit("http://mq:/v").port` is None.
    assert_eq!(parse("amqp://u:p@mq:/v").uri.authority.port, 5672);
}
