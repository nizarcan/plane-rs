// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! RabbitMQ: vhost management over the management HTTP API and a Celery
//! message drainer over AMQP.
//!
//! No worker runs during a difftest, so every task a step enqueues stays in
//! its target's vhost. After each step the harness drains every queue with
//! `basic_get` and acks, then compares the messages as part of the step's
//! side effects.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use lapin::options::{BasicAckOptions, BasicGetOptions, QueueDeclareOptions};
use lapin::types::{AMQPValue, FieldTable};
use lapin::{BasicProperties, Connection, ConnectionProperties};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Serialize;
use serde_json::{Value, json};

/// Vhost names have no reserved characters, but `%2F` must stay encoded.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

#[derive(Debug, Clone, Serialize)]
pub struct CeleryMessage {
    pub queue: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub headers: BTreeMap<String, Value>,
    pub properties: BTreeMap<String, Value>,
    pub args: Value,
    pub kwargs: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_extra: Option<Value>,
}

/// RabbitMQ management API client, used for vhost lifecycle and queue names.
pub struct RabbitAdmin {
    http: reqwest::Client,
    base: String,
    user: String,
    password: String,
}

impl RabbitAdmin {
    pub fn new(
        base: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .context("building the RabbitMQ management HTTP client")?;
        Ok(Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            user: user.into(),
            password: password.into(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub async fn create_vhost(&self, vhost: &str) -> Result<()> {
        let encoded = utf8_percent_encode(vhost, PATH_SEGMENT).to_string();
        let response = self
            .http
            .put(self.url(&format!("/api/vhosts/{encoded}")))
            .basic_auth(&self.user, Some(&self.password))
            .json(&json!({}))
            .send()
            .await
            .with_context(|| format!("creating RabbitMQ vhost {vhost:?}"))?;
        let status = response.status();
        // 201 = created, 204 = already existed.
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("RabbitMQ vhost {vhost:?}: HTTP {status}: {body}");
        }
        Ok(())
    }

    pub async fn grant_full_permissions(&self, vhost: &str, user: &str) -> Result<()> {
        let encoded_vhost = utf8_percent_encode(vhost, PATH_SEGMENT).to_string();
        let encoded_user = utf8_percent_encode(user, PATH_SEGMENT).to_string();
        let response = self
            .http
            .put(self.url(&format!("/api/permissions/{encoded_vhost}/{encoded_user}")))
            .basic_auth(&self.user, Some(&self.password))
            .json(&json!({"configure": ".*", "write": ".*", "read": ".*"}))
            .send()
            .await
            .with_context(|| format!("granting permissions on vhost {vhost:?}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("RabbitMQ permissions on {vhost:?}: HTTP {status}: {body}");
        }
        Ok(())
    }

    pub async fn delete_vhost(&self, vhost: &str) -> Result<()> {
        let encoded = utf8_percent_encode(vhost, PATH_SEGMENT).to_string();
        let response = self
            .http
            .delete(self.url(&format!("/api/vhosts/{encoded}")))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await
            .with_context(|| format!("deleting RabbitMQ vhost {vhost:?}"))?;
        let status = response.status();
        if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
            let body = response.text().await.unwrap_or_default();
            bail!("RabbitMQ deleting vhost {vhost:?}: HTTP {status}: {body}");
        }
        Ok(())
    }

    /// Sorted queue names in a vhost, or an empty list when the vhost has no
    /// queues yet (the management API returns 404 before the first declare).
    pub async fn queues(&self, vhost: &str) -> Result<Vec<String>> {
        let encoded = utf8_percent_encode(vhost, PATH_SEGMENT).to_string();
        let response = self
            .http
            .get(self.url(&format!("/api/queues/{encoded}")))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await
            .with_context(|| format!("listing queues in vhost {vhost:?}"))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("RabbitMQ queues in {vhost:?}: HTTP {status}: {body}");
        }
        let body: Value = response.json().await.context("parsing queue list")?;
        let Some(items) = body.as_array() else {
            bail!("RabbitMQ queue list was not an array");
        };
        let mut names: Vec<String> = items
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        names.sort();
        Ok(names)
    }

    pub async fn is_ready(&self) -> Result<()> {
        let response = self
            .http
            .get(self.url("/api/overview"))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await
            .context("contacting the RabbitMQ management API")?;
        if !response.status().is_success() {
            bail!(
                "RabbitMQ management API returned HTTP {}",
                response.status()
            );
        }
        Ok(())
    }
}

/// An AMQP connection used to drain queues.
pub struct CeleryDrain {
    connection: Connection,
}

impl CeleryDrain {
    pub async fn connect(amqp_url: &str) -> Result<Self> {
        let connection = Connection::connect(amqp_url, ConnectionProperties::default())
            .await
            .with_context(|| format!("connecting to RabbitMQ at {amqp_url}"))?;
        Ok(Self { connection })
    }

    /// Drain `queues` in order, returning every message.
    pub async fn drain(&self, queues: &[String]) -> Result<Vec<CeleryMessage>> {
        if queues.is_empty() {
            return Ok(Vec::new());
        }
        let channel = self
            .connection
            .create_channel()
            .await
            .context("opening an AMQP channel")?;
        let mut messages = Vec::new();
        for queue in queues {
            let declare = channel
                .queue_declare(
                    queue.as_str().into(),
                    QueueDeclareOptions {
                        passive: true,
                        ..QueueDeclareOptions::default()
                    },
                    FieldTable::default(),
                )
                .await;
            if let Err(error) = declare {
                // A queue listed by the management API can disappear between
                // the listing and the declare; an empty queue is equivalent.
                tracing::debug!("queue {queue} not declared while draining: {error}");
                continue;
            }
            loop {
                let Some(got) = channel
                    .basic_get(queue.as_str().into(), BasicGetOptions::default())
                    .await
                    .with_context(|| format!("basic_get on queue {queue}"))?
                else {
                    break;
                };
                let delivery = &got.delivery;
                messages.push(decode_delivery(queue, &delivery.properties, &delivery.data));
                delivery
                    .acker
                    .ack(BasicAckOptions::default())
                    .await
                    .with_context(|| format!("acking a message from {queue}"))?;
            }
        }
        Ok(messages)
    }
}

fn decode_delivery(queue: &str, properties: &BasicProperties, data: &[u8]) -> CeleryMessage {
    let headers = properties
        .headers()
        .as_ref()
        .map(field_table_to_json)
        .unwrap_or_default();
    let task = headers
        .get("task")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut property_map = BTreeMap::new();
    if let Some(value) = properties.content_type().as_ref() {
        property_map.insert("content_type".to_string(), json!(value.to_string()));
    }
    if let Some(value) = properties.content_encoding().as_ref() {
        property_map.insert("content_encoding".to_string(), json!(value.to_string()));
    }
    if let Some(value) = properties.delivery_mode() {
        property_map.insert("delivery_mode".to_string(), json!(value));
    }
    if let Some(value) = properties.priority() {
        property_map.insert("priority".to_string(), json!(value));
    }

    let (args, kwargs, body_extra) = match serde_json::from_slice::<Value>(data) {
        Ok(Value::Array(body)) => {
            let mut body = body;
            let extra = if body.len() > 2 {
                Some(body.remove(2))
            } else {
                None
            };
            let kwargs = if body.len() > 1 {
                body.remove(1)
            } else {
                Value::Null
            };
            let args = if body.is_empty() {
                Value::Null
            } else {
                body.remove(0)
            };
            (args, kwargs, extra)
        }
        Ok(other) => (other, Value::Null, None),
        Err(_) => (
            Value::String(String::from_utf8_lossy(data).into_owned()),
            Value::Null,
            None,
        ),
    };

    CeleryMessage {
        queue: queue.to_string(),
        task,
        headers,
        properties: property_map,
        args,
        kwargs,
        body_extra,
    }
}

fn field_table_to_json(table: &FieldTable) -> BTreeMap<String, Value> {
    table
        .into_iter()
        .map(|(key, value)| (key.to_string(), amqp_value_to_json(value)))
        .collect()
}

fn amqp_value_to_json(value: &AMQPValue) -> Value {
    match value {
        AMQPValue::Boolean(value) => json!(value),
        AMQPValue::ShortShortInt(value) => json!(value),
        AMQPValue::ShortShortUInt(value) => json!(value),
        AMQPValue::ShortInt(value) => json!(value),
        AMQPValue::ShortUInt(value) => json!(value),
        AMQPValue::LongInt(value) => json!(value),
        AMQPValue::LongUInt(value) => json!(value),
        AMQPValue::LongLongInt(value) => json!(value),
        AMQPValue::Float(value) => json!(value),
        AMQPValue::Double(value) => json!(value),
        AMQPValue::ShortString(value) => json!(value.to_string()),
        AMQPValue::LongString(value) => json!(value.to_string()),
        AMQPValue::FieldArray(values) => {
            Value::Array(values.as_slice().iter().map(amqp_value_to_json).collect())
        }
        AMQPValue::Timestamp(value) => json!(value),
        AMQPValue::FieldTable(table) => Value::Object(
            field_table_to_json(table)
                .into_iter()
                .collect::<serde_json::Map<_, _>>(),
        ),
        AMQPValue::ByteArray(bytes) => json!(crate::normalize::hex_encode(bytes.as_slice())),
        AMQPValue::DecimalValue(decimal) => {
            json!({"scale": decimal.scale, "value": decimal.value})
        }
        AMQPValue::Void => Value::Null,
    }
}

/// Parse an AMQP URI for a vhost. `vhost` is URL-encoded into the path, which
/// is how lapin and RabbitMQ expect a non-default vhost.
pub fn amqp_url_with_vhost(base: &str, vhost: &str, user: &str, password: &str) -> Result<String> {
    let scheme_end = base
        .find("://")
        .ok_or_else(|| anyhow!("AMQP URL {base:?} has no scheme"))?;
    let scheme = &base[..scheme_end];
    let rest = &base[scheme_end + 3..];
    // Drop credentials and any path already present in the base URL; the
    // caller passes the user and password separately.
    let host = rest.rsplit('@').next().unwrap_or(rest);
    let host = host.split('/').next().unwrap_or(host);
    if host.is_empty() {
        bail!("AMQP URL {base:?} has no host");
    }
    let encoded_vhost = utf8_percent_encode(vhost, PATH_SEGMENT).to_string();
    Ok(format!(
        "{scheme}://{user}:{password}@{host}/{encoded_vhost}"
    ))
}

/// Wait until the management API answers, for the harness bootstrap.
pub async fn wait_management(admin: &RabbitAdmin, timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match admin.is_ready().await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(error).context("RabbitMQ management API never became ready");
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amqp_url_encodes_the_vhost() {
        let url = amqp_url_with_vhost("amqp://127.0.0.1:55672", "dt_ab12_a", "plane", "plane")
            .expect("url");
        assert_eq!(url, "amqp://plane:plane@127.0.0.1:55672/dt_ab12_a");
        let url =
            amqp_url_with_vhost("amqp://127.0.0.1:55672/", "/", "plane", "plane").expect("url");
        assert_eq!(url, "amqp://plane:plane@127.0.0.1:55672/%2F");
    }

    #[test]
    fn amqp_url_drops_credentials_already_in_the_base() {
        let url = amqp_url_with_vhost(
            "amqp://old:creds@127.0.0.1:55672/plane",
            "dt_ab12_a",
            "plane",
            "plane",
        )
        .expect("url");
        assert_eq!(url, "amqp://plane:plane@127.0.0.1:55672/dt_ab12_a");
    }

    #[test]
    fn amqp_url_requires_a_scheme() {
        assert!(amqp_url_with_vhost("127.0.0.1:55672", "v", "u", "p").is_err());
    }

    #[test]
    fn decodes_a_celery_v2_message() {
        let body = br#"[["a", 1], {"slug": "ws"}, {"callbacks": null}]"#;
        // Build properties without headers; the body is what matters here.
        let properties = BasicProperties::default();
        let message = decode_delivery("celery", &properties, body);
        assert_eq!(message.queue, "celery");
        assert_eq!(message.args, json!(["a", 1]));
        assert_eq!(message.kwargs, json!({"slug": "ws"}));
        assert_eq!(message.body_extra, Some(json!({"callbacks": null})));
    }

    #[test]
    fn non_json_bodies_are_kept_verbatim() {
        let properties = BasicProperties::default();
        let message = decode_delivery("celery", &properties, b"not json");
        assert_eq!(message.args, json!("not json"));
        assert_eq!(message.kwargs, Value::Null);
    }
}
