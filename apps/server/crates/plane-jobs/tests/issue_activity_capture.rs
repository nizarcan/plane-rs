// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-for-byte check of the four `issue_activity` call-site shapes against
//! messages recorded from Celery 5.5.3.
//!
//! `tools/inventory/issue_activity_capture.py` publishes the real call sites
//! over kombu's in-process transport and stores properties, headers and the raw
//! body in `fixtures/issue_activity_capture.json`. Here the typed builders in
//! `plane_jobs::celery::tasks` rebuild the same messages with the same pinned
//! task ids, publish instant, `origin` and `reply_to`; the comparison includes
//! `argsrepr`/`kwargsrepr`, which render Celery's `saferepr` and therefore pin
//! the keyword order of each call site (defaults omitted).

use chrono::DateTime;
use serde_json::Value;
use uuid::Uuid;

use plane_jobs::celery::tasks::{IssueActivityBase, IssueActivityCall, issue_activity};
use plane_jobs::celery::{EncodeContext, Message, TaskArg};

const FIXTURE: &str = include_str!("fixtures/issue_activity_capture.json");

const USER: &str = "0d1e2f30-4456-4789-9abc-def012345678";
const ISSUE: &str = "11111111-2222-3333-4444-555555555555";
const PROJECT: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const EPOCH: i64 = 1_763_217_844;
const ORIGIN: &str = "http://localhost:3000";

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

fn pinned(document: &Value, field: &str) -> String {
    document
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("fixture has no {field}"))
        .to_string()
}

fn base(
    activity_type: &str,
    requested_data: TaskArg,
    current_instance: TaskArg,
) -> IssueActivityBase {
    IssueActivityBase::new(
        activity_type,
        requested_data,
        USER,
        TaskArg::str(ISSUE),
        TaskArg::str(PROJECT),
        current_instance,
        EPOCH,
    )
}

fn build(case: &str) -> Message {
    match case {
        // TASKS.md row 39 (`plane/app/views/issue/base.py:422`): create.
        "issue_activity_create" => issue_activity(IssueActivityCall::view(
            base(
                "issue.activity.created",
                TaskArg::json_text(
                    "{\"name\": \"Fix the login redirect loop\", \"priority\": \"high\", \"assignee_ids\": [\"0d1e2f30-4456-4789-9abc-def012345678\"], \"target_date\": \"2026-11-20\"}",
                ),
                TaskArg::Null,
            ),
            ORIGIN,
        )),
        // TASKS.md row 40 (`plane/app/views/issue/base.py:689`): patch.
        "issue_activity_patch" => issue_activity(IssueActivityCall::view(
            base(
                "issue.activity.updated",
                TaskArg::json_text("{\"name\": \"Fix login redirect\", \"priority\": \"medium\"}"),
                TaskArg::json_text(
                    "{\"id\": \"11111111-2222-3333-4444-555555555555\", \"name\": \"Fix login redirect loop\", \"priority\": \"high\", \"assignee_ids\": [], \"target_date\": null}",
                ),
            ),
            ORIGIN,
        )),
        // TASKS.md row 41 (`plane/app/views/issue/base.py:730`): delete,
        // `current_instance={}` and `subscriber=False` last.
        "issue_activity_delete" => issue_activity(IssueActivityCall::view_with_subscriber(
            base(
                "issue.activity.deleted",
                TaskArg::json_text("{\"issue_id\": \"11111111-2222-3333-4444-555555555555\"}"),
                TaskArg::Map(Vec::new()),
            ),
            ORIGIN,
        )),
        // TASKS.md row 44 (`plane/app/views/issue/comment.py:123`): comment
        // create.
        "comment_activity_create" => issue_activity(IssueActivityCall::view(
            base(
                "comment.activity.created",
                TaskArg::json_text(
                    "{\"id\": \"55555555-6666-7777-8888-999999999999\", \"comment_html\": \"<p>Looks good</p>\", \"comment_json\": {}, \"access\": \"INTERNAL\", \"actor\": \"0d1e2f30-4456-4789-9abc-def012345678\", \"issue\": \"11111111-2222-3333-4444-555555555555\"}",
                ),
                TaskArg::Null,
            ),
            ORIGIN,
        )),
        other => panic!("no Rust builder for case {other}"),
    }
}

#[test]
fn recorded_issue_activity_messages_are_reproduced_byte_for_byte() {
    let document = fixture();
    let origin = pinned(&document, "pinned_origin");
    let reply_to = pinned(&document, "pinned_reply_to");
    let now = DateTime::parse_from_rfc3339(&pinned(&document, "pinned_now"))
        .expect("pinned_now is RFC 3339")
        .with_timezone(&chrono::Utc);
    let messages = document["messages"]
        .as_array()
        .expect("messages is an array");
    assert_eq!(messages.len(), 4, "four recorded call-site shapes");

    for recorded in messages {
        let case = recorded["case"].as_str().expect("case");
        let message = build(case);
        assert_eq!(
            message.task(),
            recorded["task"].as_str().expect("task"),
            "task name for {case}"
        );
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
    }
}
