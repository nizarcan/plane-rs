// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The AMQP publisher: one lazy connection, one channel, Celery's defaults.
//!
//! # What is reproduced
//!
//! * Exchange `""`, routing key `celery` (`TASKS.md` §2.6).
//! * On the first publish of a connection, the same topology kombu declares
//!   through `declare=[queue]`: a durable `direct` exchange named `celery`, a
//!   durable, non-exclusive, non-auto-delete queue named `celery` with no
//!   arguments, and a binding `celery -> celery` with routing key `celery`.
//!   Verified against a fresh RabbitMQ vhost by
//!   `tools/celery/README.md`'s live check.
//! * Connection parameters from [`CeleryConfig`]: vhost percent-decoding,
//!   `localhost` -> `127.0.0.1`, heartbeats (120 s by default), a 4 s connect
//!   timeout, `amqp`/`amqps`.
//! * **No publisher confirms.** Celery does not enable them: `amqp`'s
//!   `Connection.__init__` defaults `confirm_publish=False`
//!   (`amqp/connection.py:197,260`) and neither `kombu` nor Plane sets
//!   `confirm_publish`, so `basic_publish` returns as soon as the frames are
//!   written.
//! * Celery's `task_publish_retry` policy: four attempts (the first plus
//!   `max_retries = 3`) with sleeps of 0, 0.2 and 0.4 s
//!   (`interval_start = 0`, `interval_step = 0.2`, `interval_max = 1`,
//!   `celery/app/defaults.py:278-286`). The same rendered message (same task
//!   id, same bytes) is re-published; a publish is either fully written or
//!   replaced by a retry, never duplicated by this layer — RabbitMQ does not
//!   deduplicate, exactly like Celery today.
//!
//! # What is not reproduced, and why
//!
//! * Client connection properties (`product`, `version`, …): lapin sends its
//!   own. The worker never reads them.
//! * AMQP field-table ordering: lapin's `FieldTable` is a `BTreeMap`, so
//!   application headers are sorted by key. AMQP tables are unordered;
//!   `TASKS.md` §2.7 already lists header ordering as a normalisation.
//! * TLS options other than the system trust store: `ssl_certfile` and
//!   friends are refused by [`CeleryConfig`] rather than silently ignored.
//!   Unknown query parameters are refused too: kombu would pass them to
//!   py-amqp as transport options, which this publisher cannot honour.

use std::sync::Arc;
use std::time::Duration;

use lapin::options::{
    BasicPublishOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::{AMQPValue, FieldArray, FieldTable, ShortString};
use lapin::{BasicProperties, Connection, ConnectionProperties, ExchangeKind};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::celery::args::EncodeError;
use crate::celery::broker_url::{BrokerUrlError, CeleryConfig};
use crate::celery::message::{
    DEFAULT_EXCHANGE, DEFAULT_QUEUE, DEFAULT_ROUTING_KEY, EncodeContext, EncodedMessage, Headers,
    Message,
};

/// Celery's publish retry budget: `task_publish_retry_policy["max_retries"]`.
const PUBLISH_MAX_RETRIES: u32 = 3;
/// `interval_start` (seconds) from `task_publish_retry_policy`.
const RETRY_INTERVAL_START: Duration = Duration::from_millis(0);
/// `interval_step` (seconds).
const RETRY_INTERVAL_STEP: Duration = Duration::from_millis(200);

/// A failure to publish.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// The broker URL is not usable.
    #[error(transparent)]
    BrokerUrl(#[from] BrokerUrlError),
    /// The message could not be rendered.
    #[error(transparent)]
    Encode(#[from] EncodeError),
    /// The AMQP connection, channel or publish failed.
    #[error("amqp: {0}")]
    Amqp(#[from] lapin::Error),
    /// A header value does not fit its AMQP type.
    #[error("building the AMQP properties: {0}")]
    Property(String),
}

struct State {
    // The connection must outlive the channel; both are dropped together.
    _connection: Connection,
    channel: lapin::Channel,
}

struct Inner {
    config: CeleryConfig,
    origin: String,
    reply_to: String,
    state: Mutex<Option<State>>,
}

/// A lazy, reconnect-on-failure Celery publisher.
///
/// Cloning is cheap: every clone shares the same connection and identity, so
/// one publisher belongs in the application state.
#[derive(Clone)]
pub struct CeleryPublisher {
    inner: Arc<Inner>,
}

impl CeleryPublisher {
    /// Build a publisher from a parsed broker URL. No I/O happens until the
    /// first publish.
    #[must_use]
    pub fn new(config: CeleryConfig) -> Self {
        let hostname = gethostname::gethostname();
        let hostname = hostname.to_string_lossy();
        // `celery/utils/nodenames.py:61-68`: `gen<pid>@<hostname>`.
        let origin = format!("gen{}@{hostname}", std::process::id());
        // `celery/app/base.py:912` uses `app.thread_oid`, a per-thread UUID
        // derived from the process identity. Plane has no result backend, so
        // nothing consumes the reply queue; a per-process uuid4 preserves the
        // only observable property (stable for the process's lifetime).
        let reply_to = Uuid::new_v4().hyphenated().to_string();
        Self {
            inner: Arc::new(Inner {
                config,
                origin,
                reply_to,
                state: Mutex::new(None),
            }),
        }
    }

    /// Parse `url` (Celery's `broker_url`) and build a publisher.
    ///
    /// # Errors
    ///
    /// Returns [`PublishError::BrokerUrl`] if the URL is not usable.
    pub fn from_url(url: impl Into<String>) -> Result<Self, PublishError> {
        Ok(Self::new(CeleryConfig::from_url(url)?))
    }

    /// Build a publisher from the loaded server settings
    /// ([`plane_core::Settings::broker`]).
    ///
    /// # Errors
    ///
    /// Returns [`PublishError::BrokerUrl`] if `AMQP_URL` / the `RABBITMQ_*`
    /// composition is not usable.
    pub fn from_settings(settings: &plane_core::Settings) -> Result<Self, PublishError> {
        Self::from_url(settings.broker.url.clone())
    }

    /// The `origin` header this process publishes (`gen<pid>@<hostname>`).
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.inner.origin
    }

    /// The `reply_to` property this process publishes.
    #[must_use]
    pub fn reply_to(&self) -> &str {
        &self.inner.reply_to
    }

    /// The broker URI, without credentials (for logs).
    #[must_use]
    pub fn broker_host(&self) -> String {
        format!(
            "{}://{}:{}/{}",
            self.inner.config.uri.scheme,
            self.inner.config.uri.authority.host,
            self.inner.config.uri.authority.port,
            self.inner.config.uri.vhost
        )
    }

    /// Connect and declare the queue now instead of on the first publish.
    ///
    /// # Errors
    ///
    /// Returns [`PublishError::Amqp`] if the connection or declaration fails.
    pub async fn connect(&self) -> Result<(), PublishError> {
        let mut state = self.inner.state.lock().await;
        self.ensure_connected(&mut state).await
    }

    /// Publish one typed message with a fresh task id and the current time.
    ///
    /// Returns the task id, which is also `correlation_id` and `root_id`.
    ///
    /// # Errors
    ///
    /// Returns [`PublishError`] if the message cannot be rendered or every
    /// attempt fails.
    pub async fn publish(&self, message: &Message) -> Result<Uuid, PublishError> {
        let id = Uuid::new_v4();
        let context = EncodeContext {
            id,
            now: chrono::Utc::now(),
            origin: &self.inner.origin,
            reply_to: &self.inner.reply_to,
            root_id: None,
        };
        let encoded = message.encode(&context)?;
        self.publish_encoded(&encoded).await?;
        Ok(id)
    }

    /// Publish an already-rendered message, retrying the same bytes.
    ///
    /// # Errors
    ///
    /// Returns the last [`PublishError`] after the retry budget is exhausted.
    pub async fn publish_encoded(&self, encoded: &EncodedMessage) -> Result<(), PublishError> {
        let mut attempt = 0u32;
        loop {
            match self.publish_once(encoded).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    // A failed channel is unusable; start from a fresh
                    // connection on the next attempt (`Connection::ensure`'s
                    // revive).
                    *self.inner.state.lock().await = None;
                    if attempt >= PUBLISH_MAX_RETRIES {
                        return Err(error);
                    }
                    let delay = retry_delay(attempt);
                    tracing::warn!(
                        task = encoded.task,
                        task_id = %encoded.task_id,
                        attempt,
                        delay_ms = delay.as_millis(),
                        error = %error,
                        "celery publish failed; retrying"
                    );
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    attempt += 1;
                }
            }
        }
    }

    async fn publish_once(&self, encoded: &EncodedMessage) -> Result<(), PublishError> {
        let mut guard = self.inner.state.lock().await;
        self.ensure_connected(&mut guard).await?;
        let Some(state) = guard.as_ref() else {
            return Err(PublishError::Property(
                "no channel after connect".to_string(),
            ));
        };
        let properties = basic_properties(encoded)?;
        let confirm = state
            .channel
            .basic_publish(
                ShortString::from(""),
                ShortString::from(DEFAULT_ROUTING_KEY),
                BasicPublishOptions::default(),
                &encoded.body,
                properties,
            )
            .await?;
        // Confirms are not enabled, so this resolves immediately; awaiting it
        // still surfaces a channel error raised while writing the frames.
        confirm.await?;
        Ok(())
    }

    async fn ensure_connected(&self, state: &mut Option<State>) -> Result<(), PublishError> {
        if let Some(current) = state.as_ref()
            && current.channel.status().connected()
        {
            return Ok(());
        }
        *state = None;
        let uri = self.inner.config.uri.clone();
        let connection = Connection::connect_uri(uri, ConnectionProperties::default()).await?;
        let channel = connection.create_channel().await?;
        declare_celery_topology(&channel).await?;
        *state = Some(State {
            _connection: connection,
            channel,
        });
        tracing::info!(
            broker = %self.broker_host(),
            "connected to the Celery broker"
        );
        Ok(())
    }
}

/// Declare what `kombu.Queue.declare` does for Celery's default queue:
/// exchange, queue, then binding (`kombu/entity.py:612-628,636-663`).
async fn declare_celery_topology(channel: &lapin::Channel) -> Result<(), PublishError> {
    channel
        .exchange_declare(
            ShortString::from(DEFAULT_EXCHANGE),
            ExchangeKind::Direct,
            ExchangeDeclareOptions {
                durable: true,
                passive: false,
                auto_delete: false,
                internal: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;
    channel
        .queue_declare(
            ShortString::from(DEFAULT_QUEUE),
            QueueDeclareOptions {
                passive: false,
                durable: true,
                exclusive: false,
                auto_delete: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;
    channel
        .queue_bind(
            ShortString::from(DEFAULT_QUEUE),
            ShortString::from(DEFAULT_EXCHANGE),
            ShortString::from(DEFAULT_ROUTING_KEY),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    Ok(())
}

fn basic_properties(encoded: &EncodedMessage) -> Result<BasicProperties, PublishError> {
    Ok(BasicProperties::default()
        .with_content_type(short(encoded.properties.content_type)?)
        .with_content_encoding(short(encoded.properties.content_encoding)?)
        .with_delivery_mode(encoded.properties.delivery_mode)
        .with_priority(encoded.properties.priority)
        .with_correlation_id(short(&encoded.properties.correlation_id)?)
        .with_reply_to(short(&encoded.properties.reply_to)?)
        .with_headers(header_table(&encoded.headers)?))
}

fn short(value: &str) -> Result<ShortString, PublishError> {
    ShortString::try_new(value).map_err(|error| {
        PublishError::Property(format!("{value:?} is not a short string: {error}"))
    })
}

/// The `application_headers` field table, using the AMQP types py-amqp picks
/// (`amqp/serialization.py:362-395`): strings are long strings, booleans are
/// `t`, `None` is `V`, arrays are field arrays and tables are field tables.
fn header_table(headers: &Headers) -> Result<FieldTable, PublishError> {
    let mut table = FieldTable::default();
    let long = |value: &str| AMQPValue::LongString(value.to_string().into_bytes().into());
    let maybe_long = |value: &Option<String>| match value {
        Some(value) => long(value),
        None => AMQPValue::Void,
    };
    let mut insert = |key: &str, value: AMQPValue| -> Result<(), PublishError> {
        table.insert(
            ShortString::try_new(key)
                .map_err(|error| PublishError::Property(format!("header name {key:?}: {error}")))?,
            value,
        );
        Ok(())
    };

    insert("lang", long(headers.lang))?;
    insert("task", long(headers.task))?;
    insert("id", long(&headers.id))?;
    insert("shadow", maybe_long(&headers.shadow))?;
    insert("eta", maybe_long(&headers.eta))?;
    insert("expires", maybe_long(&headers.expires))?;
    insert("group", maybe_long(&headers.group))?;
    insert(
        "group_index",
        match headers.group_index {
            Some(value) => AMQPValue::LongInt(
                i32::try_from(value)
                    .map_err(|_| PublishError::Property("group_index".to_string()))?,
            ),
            None => AMQPValue::Void,
        },
    )?;
    insert("retries", int(headers.retries)?)?;
    let mut timelimit = FieldArray::default();
    for limit in headers.timelimit {
        match limit {
            Some(value) => timelimit.push(int(value)?),
            None => timelimit.push(AMQPValue::Void),
        }
    }
    insert("timelimit", AMQPValue::FieldArray(timelimit))?;
    insert("root_id", long(&headers.root_id))?;
    insert("parent_id", maybe_long(&headers.parent_id))?;
    insert("argsrepr", long(&headers.argsrepr))?;
    insert("kwargsrepr", long(&headers.kwargsrepr))?;
    insert("origin", long(&headers.origin))?;
    insert("ignore_result", AMQPValue::Boolean(headers.ignore_result))?;
    insert("replaced_task_nesting", int(headers.replaced_task_nesting)?)?;
    insert(
        "stamped_headers",
        match &headers.stamped_headers {
            Some(values) => {
                let mut array = FieldArray::default();
                for value in values {
                    array.push(long(value));
                }
                AMQPValue::FieldArray(array)
            }
            None => AMQPValue::Void,
        },
    )?;
    let mut stamps = FieldTable::default();
    for (stamp, value) in &headers.stamps {
        stamps.insert(
            ShortString::try_new(stamp.as_str())
                .map_err(|error| PublishError::Property(format!("stamp {stamp:?}: {error}")))?,
            long(value),
        );
    }
    insert("stamps", AMQPValue::FieldTable(stamps))?;
    Ok(table)
}

/// py-amqp writes a Python `int` as a 32-bit signed integer when it fits and
/// a 64-bit one otherwise (`amqp/serialization.py:372-376`).
fn int(value: i64) -> Result<AMQPValue, PublishError> {
    if let Ok(value) = i32::try_from(value) {
        Ok(AMQPValue::LongInt(value))
    } else {
        Ok(AMQPValue::LongLongInt(value))
    }
}

/// Kombu's `task_publish_retry_policy`: `interval_start + interval_step *
/// attempt`, capped at `interval_max` (all three are 0 s / 0.2 s / 1 s here,
/// so the first retry is immediate and the later ones are 200 ms and 400 ms).
fn retry_delay(attempt: u32) -> Duration {
    RETRY_INTERVAL_START + RETRY_INTERVAL_STEP * attempt
}

#[cfg(test)]
mod tests {
    use super::retry_delay;
    use std::time::Duration;

    #[test]
    fn publish_retry_schedule_matches_kombu() {
        // `fxrange(0, 1, 0.2)` for the three retries Celery allows.
        assert_eq!(retry_delay(0), Duration::from_millis(0));
        assert_eq!(retry_delay(1), Duration::from_millis(200));
        assert_eq!(retry_delay(2), Duration::from_millis(400));
    }
}
