// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Live RabbitMQ test, gated on `PLANE_RS_TEST_AMQP_URL`.
//!
//! It publishes one rendered message through the real lapin transport, drains
//! it back, and compares the delivered properties, headers and body with the
//! bytes that were sent. That proves the transport path (exchange `""`,
//! routing key `celery`, queue declaration, AMQP property/header encoding)
//! does not alter the message.
//!
//! The vhost must be disposable: the test drains every message on `celery`.
//! `tools/celery/live_check.sh` creates one per run.
//!
//! With `PLANE_RS_REQUIRE_DB_TESTS=1` (the CI convention used by
//! `plane-db`/`plane-query`) a missing URL fails instead of skipping.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use lapin::options::{BasicAckOptions, BasicGetOptions, QueueDeclareOptions};
use lapin::types::{AMQPValue, FieldTable};
use lapin::{BasicProperties, Connection, ConnectionProperties};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::celery::message::{DEFAULT_QUEUE, EncodeContext};
use crate::celery::publisher::CeleryPublisher;
use crate::celery::tasks::{
    RecentVisitedTask, get_asset_object_metadata, process_logs, recent_visited_task,
};
use crate::celery::{TaskArg, test_cases};

const PINNED_ORIGIN: &str = "gen4242@celery-golden";
const PINNED_REPLY_TO: &str = "0f0f0f0f-0000-4000-8000-000000000000";

/// Both tests publish to the same `celery` queue in a disposable vhost; run
/// them one at a time so each can assert an exact delivery count.
static LIVE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn amqp_url() -> Option<String> {
    match std::env::var("PLANE_RS_TEST_AMQP_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            if std::env::var("PLANE_RS_REQUIRE_DB_TESTS").as_deref() == Ok("1") {
                panic!("PLANE_RS_TEST_AMQP_URL is required when PLANE_RS_REQUIRE_DB_TESTS=1");
            }
            None
        }
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
        AMQPValue::LongString(value) => json!(String::from_utf8_lossy(value.as_bytes())),
        AMQPValue::FieldArray(values) => {
            Value::Array(values.as_slice().iter().map(amqp_value_to_json).collect())
        }
        AMQPValue::FieldTable(table) => Value::Object(
            field_table_to_json(table)
                .into_iter()
                .collect::<serde_json::Map<_, _>>(),
        ),
        AMQPValue::Void => Value::Null,
        other => panic!("unexpected header value type: {other:?}"),
    }
}

fn property_map(properties: &BasicProperties) -> Value {
    json!({
        "content_type": properties.content_type().as_ref().map(ToString::to_string),
        "content_encoding": properties.content_encoding().as_ref().map(ToString::to_string),
        "delivery_mode": properties.delivery_mode(),
        "priority": properties.priority(),
        "correlation_id": properties.correlation_id().as_ref().map(ToString::to_string),
        "reply_to": properties.reply_to().as_ref().map(ToString::to_string),
    })
}

#[tokio::test]
async fn publishes_messages_the_broker_decodes_unchanged() {
    let Some(url) = amqp_url() else {
        eprintln!("skipping: PLANE_RS_TEST_AMQP_URL is not set");
        return;
    };
    let _guard = LIVE_LOCK.lock().await;
    // Leave no message behind from an earlier run.
    drain_all(&url).await;
    let publisher = CeleryPublisher::from_url(url.clone()).expect("broker URL parses");
    let now = DateTime::parse_from_rfc3339("2026-11-15T17:24:04.144120+00:00")
        .expect("fixed instant")
        .with_timezone(&Utc);

    // One message per protocol shape: kwargs with a UUID envelope, a JSON
    // string, a positional argument, and a countdown (eta).
    let messages = vec![
        (
            Uuid::from_u128(0x00000000_0000_4000_8000_000000000101),
            recent_visited_task(RecentVisitedTask::ProjectKeyed {
                slug: "acme".to_string(),
                project_id: Some(TaskArg::uuid(Uuid::from_u128(
                    0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee,
                ))),
                entity_name: "project".to_string(),
                entity_identifier: TaskArg::uuid(Uuid::from_u128(
                    0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee,
                )),
                user_id: TaskArg::uuid(Uuid::from_u128(0x0d1e2f30_4456_4789_9abc_def012345678)),
            }),
        ),
        (
            Uuid::from_u128(0x00000000_0000_4000_8000_000000000102),
            process_logs(TaskArg::map([
                ("token_identifier", TaskArg::str("b3f1c0d2e3a4")),
                ("path", TaskArg::str("/api/v1/workspaces/acme/projects/")),
                ("method", TaskArg::str("GET")),
                ("response_code", TaskArg::Int(200)),
            ])),
        ),
        (
            Uuid::from_u128(0x00000000_0000_4000_8000_000000000103),
            get_asset_object_metadata("99999999-8888-7777-6666-555555555555".to_string()),
        ),
        (
            Uuid::from_u128(0x00000000_0000_4000_8000_000000000104),
            crate::celery::tasks::sync_issue_version(
                crate::celery::tasks::BackfillBatch::Continue {
                    batch_size: 5000,
                    offset: 5000,
                    countdown: 300,
                },
            ),
        ),
    ];

    let mut encoded = Vec::new();
    for (id, message) in &messages {
        let rendered = message
            .encode(&EncodeContext {
                id: *id,
                now,
                origin: PINNED_ORIGIN,
                reply_to: PINNED_REPLY_TO,
                root_id: None,
            })
            .expect("message renders");
        publisher
            .publish_encoded(&rendered)
            .await
            .expect("publish succeeds");
        encoded.push(rendered);
    }

    // Drain with lapin and compare.
    let received = drain_all(&url).await;
    assert_eq!(received.len(), encoded.len(), "drained message count");

    for expected in &encoded {
        let delivery = received
            .iter()
            .find(|delivery| {
                delivery
                    .properties
                    .correlation_id()
                    .as_ref()
                    .is_some_and(|id| id.to_string() == expected.task_id.hyphenated().to_string())
            })
            .unwrap_or_else(|| panic!("no delivery for task {}", expected.task_id));
        assert_eq!(
            delivery.data, expected.body,
            "body bytes for {}",
            expected.task
        );
        let headers = delivery
            .properties
            .headers()
            .as_ref()
            .map(field_table_to_json)
            .unwrap_or_default();
        assert_eq!(
            Value::Object(headers.into_iter().collect()),
            expected.headers.to_json(),
            "headers for {}",
            expected.task
        );
        assert_eq!(
            property_map(&delivery.properties),
            json!({
                "content_type": expected.properties.content_type,
                "content_encoding": expected.properties.content_encoding,
                "delivery_mode": expected.properties.delivery_mode,
                "priority": expected.properties.priority,
                "correlation_id": expected.properties.correlation_id,
                "reply_to": expected.properties.reply_to,
            }),
            "properties for {}",
            expected.task
        );
        // `eta` is ISO-8601 with microseconds and `+00:00`.
        if let Some(eta) = &expected.headers.eta {
            assert!(
                eta.ends_with("+00:00") && eta.contains('T') && eta.len() > 25,
                "eta shape for {}: {eta:?}",
                expected.task
            );
        }
    }
}

#[tokio::test]
async fn golden_cases_can_be_published() {
    let Some(url) = amqp_url() else {
        eprintln!("skipping: PLANE_RS_TEST_AMQP_URL is not set");
        return;
    };
    let _guard = LIVE_LOCK.lock().await;
    // A smoke test over every representative case: each renders and the
    // broker accepts it. The golden test already proved the bytes; this
    // proves the transport accepts the largest/nested shapes too.
    let publisher = CeleryPublisher::from_url(url.clone()).expect("broker URL parses");
    for case in test_cases::case_ids() {
        let message =
            test_cases::build(case).unwrap_or_else(|| panic!("no Rust builder for case {case}"));
        publisher
            .publish(&message)
            .await
            .unwrap_or_else(|error| panic!("publishing {case}: {error}"));
    }
    assert_eq!(
        drain_all(&url).await.len(),
        test_cases::case_ids().len(),
        "every published case reaches the broker"
    );
}

/// Drain the `celery` queue with lapin, acking everything. The vhost is
/// disposable, so leaving unrelated messages would make test order matter.
async fn drain_all(url: &str) -> Vec<lapin::message::Delivery> {
    let connection = Connection::connect(url, ConnectionProperties::default())
        .await
        .expect("drain connects");
    let channel = connection.create_channel().await.expect("drain channel");
    channel
        .queue_declare(
            DEFAULT_QUEUE.into(),
            QueueDeclareOptions {
                passive: true,
                ..QueueDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await
        .expect("celery queue exists");
    let mut received = Vec::new();
    while let Some(delivery) = channel
        .basic_get(DEFAULT_QUEUE.into(), BasicGetOptions::default())
        .await
        .expect("basic_get")
    {
        delivery
            .delivery
            .acker
            .ack(BasicAckOptions::default())
            .await
            .expect("ack");
        received.push(delivery.delivery);
    }
    let _ = connection.close(0, "done".into()).await;
    received
}
