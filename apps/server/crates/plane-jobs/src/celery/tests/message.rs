// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Message rendering independent of the golden corpus.

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use crate::celery::args::TaskArg;
use crate::celery::message::{EncodeContext, Message, py_isoformat, task_name};
use crate::celery::tasks::{IssueActivityBase, IssueActivityCall, delete_api_logs, issue_activity};

fn context<'a>(id: Uuid, origin: &'a str, reply_to: &'a str) -> EncodeContext<'a> {
    EncodeContext {
        id,
        now: Utc
            .with_ymd_and_hms(2026, 11, 15, 17, 24, 4)
            .single()
            .expect("valid instant")
            + chrono::TimeDelta::microseconds(144_120),
        origin,
        reply_to,
        root_id: None,
    }
}

fn encoded(message: &Message) -> crate::celery::EncodedMessage {
    message
        .encode(&context(
            Uuid::from_u128(0x00000000_0000_4000_8000_000000000001),
            "gen1@host",
            "0f0f0f0f-0000-4000-8000-000000000000",
        ))
        .expect("message renders")
}

#[test]
fn a_no_argument_message_has_celery_s_exact_body() {
    let message = delete_api_logs();
    let encoded = encoded(&message);
    assert_eq!(
        std::str::from_utf8(&encoded.body).expect("utf-8"),
        r#"[[], {}, {"callbacks": null, "errbacks": null, "chain": null, "chord": null}]"#
    );
    assert_eq!(encoded.headers.task, task_name::DELETE_API_LOGS);
    assert_eq!(encoded.headers.id, "00000000-0000-4000-8000-000000000001");
    assert_eq!(encoded.headers.root_id, encoded.headers.id);
    assert_eq!(encoded.properties.correlation_id, encoded.headers.id);
    assert_eq!(encoded.properties.content_type, "application/json");
    assert_eq!(encoded.properties.content_encoding, "utf-8");
    assert_eq!(encoded.properties.delivery_mode, 2);
    assert_eq!(encoded.properties.priority, 0);
    assert_eq!(encoded.headers.retries, 0);
    assert_eq!(encoded.headers.timelimit, [None, None]);
    assert_eq!(encoded.headers.kwargsrepr, "{}");
    assert_eq!(encoded.headers.argsrepr, "()");
}

#[test]
fn zero_countdown_publishes_no_eta_and_a_negative_one_publishes_an_eta() {
    let base = IssueActivityBase {
        activity_type: "issue.activity.updated".to_string(),
        requested_data: TaskArg::json_text(r#"{"a": 1}"#),
        actor_id: "0d1e2f30-4456-4789-9abc-def012345678".to_string(),
        issue_id: TaskArg::str("11111111-2222-3333-4444-555555555555"),
        project_id: TaskArg::str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        current_instance: TaskArg::Null,
        epoch: 1,
    };
    let view = || issue_activity(IssueActivityCall::view(base.clone(), "http://localhost"));

    assert_eq!(encoded(&view().with_countdown(0)).headers.eta, None);
    assert_eq!(
        encoded(&view().with_countdown(300)).headers.eta.as_deref(),
        Some("2026-11-15T17:29:04.144120+00:00")
    );
    assert_eq!(
        encoded(&view().with_countdown(-5)).headers.eta.as_deref(),
        Some("2026-11-15T17:23:59.144120+00:00")
    );
}

#[test]
fn eta_omits_zero_microseconds() {
    let instant = Utc
        .with_ymd_and_hms(2026, 11, 15, 17, 24, 4)
        .single()
        .expect("valid instant");
    assert_eq!(py_isoformat(instant), "2026-11-15T17:24:04+00:00");
}

#[test]
fn django_json_text_matches_django_json_encoder() {
    let value = plane_serde::pyjson::loads(br#"{"a": 1.0, "b": [true, null], "c": "x"}"#, "utf-8")
        .expect("valid JSON");
    let text = TaskArg::json_text_from(&value).expect("renderable");
    assert_eq!(
        text,
        TaskArg::json_text(r#"{"a": 1.0, "b": [true, null], "c": "x"}"#)
    );
}

#[test]
fn countdown_is_not_a_body_field() {
    let message = delete_api_logs().with_countdown(300);
    let encoded = encoded(&message);
    let body = std::str::from_utf8(&encoded.body).expect("utf-8");
    assert!(!body.contains("countdown"));
    assert_eq!(
        encoded.headers.eta.as_deref(),
        Some("2026-11-15T17:29:04.144120+00:00")
    );
}
