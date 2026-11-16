// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-for-byte golden test against messages recorded from Celery 5.5.3.
//!
//! `tools/celery/record.py` publishes the cases through the real Celery task
//! wrappers over kombu's in-memory transport and stores the properties,
//! headers and raw body bytes in `tools/celery/golden/messages.json`. Here the
//! Rust typed API builds the same message, with the same pinned task id,
//! publish instant, `origin` and `reply_to`, and compares everything —
//! including `argsrepr`/`kwargsrepr`, which exercise Celery's `saferepr`
//! (not Python's `repr()`).

use chrono::DateTime;
use serde_json::Value;
use uuid::Uuid;

use crate::celery::message::EncodeContext;
use crate::celery::test_cases;

const GOLDEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/celery/golden/messages.json"
));

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden/messages.json parses")
}

fn pinned(document: &Value, field: &str) -> String {
    document
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("golden document has no {field}"))
        .to_string()
}

#[test]
fn every_recorded_case_has_a_rust_builder() {
    let document = golden();
    let messages = document["messages"]
        .as_array()
        .expect("messages is an array");
    assert!(
        messages.len() >= 40,
        "expected at least 40 representative calls, found {}",
        messages.len()
    );

    let mut recorded: Vec<&str> = messages
        .iter()
        .map(|message| message["case"].as_str().expect("case is a string"))
        .collect();
    recorded.sort_unstable();

    let mut known = test_cases::case_ids().to_vec();
    known.sort_unstable();
    assert_eq!(
        recorded, known,
        "case ids differ between record.py and Rust"
    );

    for case in recorded {
        assert!(
            test_cases::build(case).is_some(),
            "Rust has no builder for case {case}"
        );
    }
}

#[test]
fn case_order_matches_the_recorder_is_derived_task_ids() {
    let document = golden();
    let ids = test_cases::case_ids();
    for message in document["messages"].as_array().expect("messages") {
        let case = message["case"].as_str().expect("case");
        let index = ids
            .iter()
            .position(|known| *known == case)
            .unwrap_or_else(|| panic!("case {case} is not in case_ids()"));
        let expected = format!("00000000-0000-4000-8000-{:012}", index + 1);
        assert_eq!(
            message["task_id"].as_str(),
            Some(expected.as_str()),
            "task id for {case} is derived from its position in cases.py"
        );
    }
}

#[test]
fn recorded_messages_are_reproduced_byte_for_byte() {
    let document = golden();
    let pinned_now = pinned(&document, "pinned_now");
    let origin = pinned(&document, "pinned_origin");
    let reply_to = pinned(&document, "pinned_reply_to");
    let now = DateTime::parse_from_rfc3339(&pinned_now)
        .expect("pinned_now is RFC 3339")
        .with_timezone(&chrono::Utc);
    let messages = document["messages"]
        .as_array()
        .expect("messages is an array");

    let mut checked = 0u32;
    for recorded in messages {
        let case = recorded["case"].as_str().expect("case");
        let message =
            test_cases::build(case).unwrap_or_else(|| panic!("no Rust builder for case {case}"));
        let task = recorded["task"].as_str().expect("task");
        assert_eq!(message.task(), task, "task name for {case}");

        let task_id = recorded["task_id"]
            .as_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(|| panic!("task id for {case}"));
        let encoded = message
            .encode(&EncodeContext {
                id: task_id,
                now,
                origin: &origin,
                reply_to: &reply_to,
                root_id: None,
            })
            .unwrap_or_else(|error| panic!("encoding {case}: {error}"));

        let mut expected = recorded.clone();
        if let Some(object) = expected.as_object_mut() {
            object.remove("case");
            object.remove("call_site");
            object.remove("body_bytes");
        }
        assert_eq!(
            encoded.to_json(),
            expected,
            "message for {case} differs from the Python capture"
        );
        checked += 1;
    }
    assert_eq!(checked as usize, messages.len());
}
