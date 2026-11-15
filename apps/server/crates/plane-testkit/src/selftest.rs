// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest selftest`: exercises the normaliser and the comparator on
//! synthetic cases, with no network or container involved.
//!
//! Two things are proven here:
//! 1. every injected difference is detected, in the right part; and
//! 2. every legitimately volatile variation is accepted.
//!
//! A harness that passes case 2 but fails case 1 would be vacuous; the
//! selftest fails unless both hold.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::celery::CeleryMessage;
use crate::compare::{
    Difference, FormattingStatus, check_drf_formatting, compare_bodies, compare_celery, compare_db,
    compare_formatting, compare_headers, compare_status, drf_compact,
};
use crate::config::{DbRule, NamedRule, NormalizeRules, ValueMode};
use crate::db::{DbDiff, TableDiff, diff_snapshots};
use crate::normalize::{BodyValue, NormalizedBody, TargetNormalizer};
use crate::timeparse::parse_timestamp;

struct Checks {
    count: usize,
}

impl Checks {
    fn new() -> Self {
        Self { count: 0 }
    }

    fn ok(&mut self, label: &str, condition: bool) -> Result<()> {
        self.count += 1;
        if condition {
            Ok(())
        } else {
            bail!("selftest failed: {label}")
        }
    }

    fn accepted(&mut self, label: &str, differences: &[Difference]) -> Result<()> {
        self.count += 1;
        if differences.is_empty() {
            Ok(())
        } else {
            bail!(
                "selftest failed: {label} produced {} difference(s): {:#?}",
                differences.len(),
                differences
            );
        }
    }

    fn detected(&mut self, label: &str, part: &str, differences: &[Difference]) -> Result<()> {
        self.count += 1;
        if differences.is_empty() {
            return Err(anyhow::anyhow!(
                "selftest failed: {label} was not detected (vacuous comparison)"
            ));
        }
        if !differences.iter().any(|difference| difference.part == part) {
            bail!(
                "selftest failed: {label} was detected but not in part {part:?}: {:?}",
                differences
                    .iter()
                    .map(|difference| difference.part.clone())
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }
}

fn rules() -> Arc<NormalizeRules> {
    Arc::new(NormalizeRules {
        ignore_headers: vec![
            "date".to_string(),
            "server".to_string(),
            "content-length".to_string(),
        ],
        volatile_cookies: vec![
            NamedRule {
                name: "session-id".to_string(),
                why: "random session key".to_string(),
                mode: ValueMode::Volatile,
            },
            NamedRule {
                name: "csrftoken".to_string(),
                why: "random masked csrf secret".to_string(),
                mode: ValueMode::Volatile,
            },
        ],
        volatile_json_keys: vec![
            NamedRule {
                name: "csrf_token".to_string(),
                why: "masked csrf token".to_string(),
                mode: ValueMode::Volatile,
            },
            NamedRule {
                name: "token".to_string(),
                why: "random api token, empty until generated".to_string(),
                mode: ValueMode::VolatileIfNonEmpty,
            },
            NamedRule {
                name: "epoch".to_string(),
                why: "whole-second wall clock".to_string(),
                mode: ValueMode::UnixTime,
            },
        ],
        volatile_celery_headers: vec![NamedRule {
            name: "id".to_string(),
            why: "celery task id".to_string(),
            mode: ValueMode::Volatile,
        }],
        volatile_db_columns: vec![
            DbRule {
                table: "sessions".to_string(),
                column: "session_key".to_string(),
                why: "random key".to_string(),
                mode: ValueMode::Volatile,
            },
            DbRule {
                table: "sessions".to_string(),
                column: "expire_date".to_string(),
                why: "now + cookie age".to_string(),
                mode: ValueMode::RelativeTime,
            },
            DbRule {
                table: "users".to_string(),
                column: "password".to_string(),
                why: "salted hash".to_string(),
                mode: ValueMode::PasswordPrefix,
            },
            DbRule {
                table: "workspaces".to_string(),
                column: "background_color".to_string(),
                why: "random default colour".to_string(),
                mode: ValueMode::RandomColor,
            },
        ],
        timestamp_slack_secs: 2,
        db_schemas: vec!["public".to_string()],
        db_exclude_tables: vec![],
    })
}

fn normalizer(base_url: &str) -> TargetNormalizer {
    TargetNormalizer::new(rules(), base_url, Arc::new(HashSet::new()))
}

fn time(text: &str) -> DateTime<Utc> {
    parse_timestamp(text).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

fn body(value: Value) -> NormalizedBody {
    NormalizedBody {
        value: BodyValue::Json(value),
        modified: false,
    }
}

fn celery_message(task: &str, args: Value) -> CeleryMessage {
    CeleryMessage {
        queue: "celery".to_string(),
        task: Some(task.to_string()),
        headers: BTreeMap::new(),
        properties: BTreeMap::new(),
        args,
        kwargs: json!({}),
        body_extra: None,
    }
}

fn db_diff(table: &str, inserted: Vec<Value>, deleted: Vec<Value>, updated: Vec<Value>) -> DbDiff {
    let mut diff = DbDiff::default();
    diff.tables.insert(
        table.to_string(),
        TableDiff {
            inserted,
            deleted,
            updated,
        },
    );
    diff
}

pub fn run() -> Result<()> {
    let mut checks = Checks::new();
    println!("selftest: normaliser");

    // -- UUID placeholding -------------------------------------------------
    let uuid_a = "11111111-1111-4111-8111-111111111111";
    let uuid_b = "22222222-2222-4222-8222-222222222222";
    let uuid_c = "33333333-3333-4333-8333-333333333333";
    let uuid_d = "44444444-4444-4444-8444-444444444444";
    let uuid_e = "55555555-5555-4555-8555-555555555555";

    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    let start = time("2026-11-15T20:00:00Z");
    a.start_step(start, start);
    b.start_step(start, start);

    let headers_a = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Location".to_string(),
            format!("http://127.0.0.1:18000/items/{uuid_a}/"),
        ),
        ("X-Request-Id".to_string(), uuid_b.to_string()),
    ];
    let headers_b = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Location".to_string(),
            format!("http://127.0.0.1:18001/items/{uuid_c}/"),
        ),
        ("X-Request-Id".to_string(), uuid_d.to_string()),
    ];
    let norm_a = a.normalize_headers(&headers_a);
    let norm_b = b.normalize_headers(&headers_b);
    checks.accepted("header UUID + base URL", &compare_headers(&norm_a, &norm_b))?;
    checks.ok(
        "Location placeholder is <id:1>",
        norm_a[1].1 == "<base>/items/<id:1>/" && norm_b[1].1 == "<base>/items/<id:1>/",
    )?;

    let body_a = a.normalize_body(
        format!(r#"{{"id":"{uuid_e}","url":"http://127.0.0.1:18000/x/{uuid_e}"}}"#).as_bytes(),
        true,
    );
    let body_b = b.normalize_body(
        format!(r#"{{"id":"{uuid_e}","url":"http://127.0.0.1:18001/x/{uuid_e}"}}"#).as_bytes(),
        true,
    );
    checks.accepted("body UUID + base URL", &compare_bodies(&body_a, &body_b))?;
    checks.ok(
        "body id placeholder follows headers",
        body_a.pretty().contains("<id:3>") && body_b.pretty().contains("<id:3>"),
    )?;

    // Known UUIDs (seeded rows) are left alone.
    let known = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_string();
    let mut with_known = TargetNormalizer::new(
        rules(),
        "http://x",
        Arc::new(HashSet::from([known.clone()])),
    );
    with_known.start_step(start, start);
    checks.ok(
        "baseline UUIDs are not placeholdered",
        with_known.normalize_text(&known) == known,
    )?;

    // -- timestamps --------------------------------------------------------
    let window_a = "2026-11-15T20:00:00Z";
    let window_b = "2026-11-15T20:00:02Z";
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(time(window_a), time(window_a));
    b.start_step(time(window_b), time(window_b));
    let ts_a = a.normalize_body(br#"{"created_at":"2026-11-15T20:00:01.123456Z"}"#, true);
    let ts_b = b.normalize_body(br#"{"created_at":"2026-11-15T20:00:02.654321Z"}"#, true);
    checks.accepted("in-window timestamps", &compare_bodies(&ts_a, &ts_b))?;
    checks.ok(
        "timestamp rendered as <now>",
        ts_a.pretty().contains("<now>") && ts_b.pretty().contains("<now>"),
    )?;

    let older = "2024-01-01T00:00:00Z";
    checks.ok(
        "out-of-window timestamps are compared exactly",
        a.normalize_text(older) == older,
    )?;
    let mutated = a.normalize_body(br#"{"created_at":"2024-01-01T00:00:00Z"}"#, true);
    let mutated2 = b.normalize_body(br#"{"created_at":"2024-01-01T00:00:01Z"}"#, true);
    checks.detected(
        "out-of-window timestamp difference",
        "body",
        &compare_bodies(&mutated, &mutated2),
    )?;

    // -- named volatile values --------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let cookies_a = a.normalize_headers(&[(
        "Set-Cookie".to_string(),
        "session-id=aaa; Path=/; HttpOnly; SameSite=Lax".to_string(),
    )]);
    let cookies_b = b.normalize_headers(&[(
        "Set-Cookie".to_string(),
        "session-id=bbb; Path=/; HttpOnly; SameSite=Lax".to_string(),
    )]);
    checks.accepted(
        "volatile session cookie value",
        &compare_headers(&cookies_a, &cookies_b),
    )?;

    let cookies_b_path = b.normalize_headers(&[(
        "Set-Cookie".to_string(),
        "session-id=bbb; Path=/app; HttpOnly; SameSite=Lax".to_string(),
    )]);
    checks.detected(
        "changed cookie attribute",
        "headers",
        &compare_headers(&cookies_a, &cookies_b_path),
    )?;

    let csrf_a = a.normalize_body(br#"{"csrf_token":"aaa","x":1}"#, true);
    let csrf_b = b.normalize_body(br#"{"csrf_token":"bbb","x":1}"#, true);
    checks.accepted("volatile csrf_token", &compare_bodies(&csrf_a, &csrf_b))?;

    let mut db_a = db_diff(
        "public.sessions",
        vec![json!({"session_key": "aaaa", "user_id": "7"})],
        vec![],
        vec![],
    );
    let mut db_b = db_diff(
        "public.sessions",
        vec![json!({"session_key": "bbbb", "user_id": "7"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut db_a);
    b.normalize_db_diff(&mut db_b);
    checks.accepted("volatile session_key column", &compare_db(&db_a, &db_b))?;

    let mut expiry_a = db_diff(
        "public.sessions",
        vec![json!({"expire_date": "2026-11-22T20:00:00.101+00:00"})],
        vec![],
        vec![],
    );
    let mut expiry_b = db_diff(
        "public.sessions",
        vec![json!({"expire_date": "2026-11-22T20:00:00.987+00:00"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut expiry_a);
    b.normalize_db_diff(&mut expiry_b);
    checks.accepted(
        "session expiry compared as a relative age",
        &compare_db(&expiry_a, &expiry_b),
    )?;
    let mut expiry_bad = db_diff(
        "public.sessions",
        vec![json!({"expire_date": "2026-11-23T20:00:00.987+00:00"})],
        vec![],
        vec![],
    );
    b.normalize_db_diff(&mut expiry_bad);
    checks.detected(
        "a different session age is still detected",
        "db",
        &compare_db(&expiry_a, &expiry_bad),
    )?;

    let mut hash_a = db_diff(
        "public.users",
        vec![json!({"password": "pbkdf2_sha256$1200000$saltA$hashA"})],
        vec![],
        vec![],
    );
    let mut hash_b = db_diff(
        "public.users",
        vec![json!({"password": "pbkdf2_sha256$1200000$saltB$hashB"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut hash_a);
    b.normalize_db_diff(&mut hash_b);
    checks.accepted("password salt and hash", &compare_db(&hash_a, &hash_b))?;

    let mut hash_bad = db_diff(
        "public.users",
        vec![json!({"password": "pbkdf2_sha1$500000$salt$hash"})],
        vec![],
        vec![],
    );
    b.normalize_db_diff(&mut hash_bad);
    checks.detected(
        "password algorithm/iteration change",
        "db",
        &compare_db(&hash_a, &hash_bad),
    )?;

    let mut colour_a = db_diff(
        "public.workspaces",
        vec![json!({"background_color": "#a1b2c3"})],
        vec![],
        vec![],
    );
    let mut colour_b = db_diff(
        "public.workspaces",
        vec![json!({"background_color": "#d4e5f6"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut colour_a);
    b.normalize_db_diff(&mut colour_b);
    checks.accepted("random default colour", &compare_db(&colour_a, &colour_b))?;
    let mut colour_bad = db_diff(
        "public.workspaces",
        vec![json!({"background_color": "not-a-colour"})],
        vec![],
        vec![],
    );
    b.normalize_db_diff(&mut colour_bad);
    checks.detected(
        "constant colour still compared",
        "db",
        &compare_db(&colour_a, &colour_bad),
    )?;

    // -- DB diffs ----------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);

    let insert_a = db_diff(
        "public.issues",
        vec![json!({"id": uuid_a, "name": "Work item", "created_at": "2026-11-15T20:00:00.1Z"})],
        vec![],
        vec![],
    );
    let insert_b = db_diff(
        "public.issues",
        vec![json!({"id": uuid_b, "name": "Work item", "created_at": "2026-11-15T20:00:00.9Z"})],
        vec![],
        vec![],
    );
    let mut insert_a = insert_a;
    let mut insert_b = insert_b;
    a.normalize_db_diff(&mut insert_a);
    b.normalize_db_diff(&mut insert_b);
    checks.accepted(
        "database insert with generated id and timestamp",
        &compare_db(&insert_a, &insert_b),
    )?;

    let insert_b_extra = db_diff(
        "public.issues",
        vec![
            json!({"id": uuid_b, "name": "Work item", "created_at": "2026-11-15T20:00:00.9Z"}),
            json!({"id": uuid_c, "name": "Unexpected", "created_at": "2026-11-15T20:00:00.9Z"}),
        ],
        vec![],
        vec![],
    );
    let mut insert_b_extra = insert_b_extra;
    b.normalize_db_diff(&mut insert_b_extra);
    checks.detected(
        "extra database row",
        "db",
        &compare_db(&insert_a, &insert_b_extra),
    )?;

    let delete_a = db_diff(
        "public.issues",
        vec![],
        vec![json!({"id": uuid_a, "name": "Gone"})],
        vec![],
    );
    let mut delete_a = delete_a;
    a.normalize_db_diff(&mut delete_a);
    checks.detected(
        "missing database row",
        "db",
        &compare_db(&delete_a, &db_diff("public.issues", vec![], vec![], vec![])),
    )?;

    let update_a = db_diff(
        "public.issues",
        vec![],
        vec![],
        vec![json!({"pk": [uuid_a], "changed": {"name": {"a": "old", "b": "new"}}})],
    );
    let update_b = db_diff(
        "public.issues",
        vec![],
        vec![],
        vec![json!({"pk": [uuid_b], "changed": {"name": {"a": "old", "b": "new"}}})],
    );
    let mut update_a = update_a;
    let mut update_b = update_b;
    a.normalize_db_diff(&mut update_a);
    b.normalize_db_diff(&mut update_b);
    checks.accepted(
        "database update with generated id",
        &compare_db(&update_a, &update_b),
    )?;

    let update_c = db_diff(
        "public.issues",
        vec![],
        vec![],
        vec![json!({"pk": [uuid_b], "changed": {"name": {"a": "old", "b": "different"}}})],
    );
    let mut update_c = update_c;
    b.normalize_db_diff(&mut update_c);
    checks.detected(
        "database update with a different value",
        "db",
        &compare_db(&update_a, &update_c),
    )?;

    // -- snapshot diff, end to end ----------------------------------------
    let mut before = crate::db::DbSnapshot::default();
    let after = before.clone();
    checks.ok(
        "identical snapshots have an empty diff",
        diff_snapshots(&before, &after).is_empty(),
    )?;
    before
        .tables
        .insert("public.t".to_string(), BTreeMap::new());

    // -- Celery ------------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let mut message_a = celery_message(
        "plane.tasks.issue_activity",
        json!([uuid_a, {"id": uuid_b}]),
    );
    message_a.headers.insert("id".to_string(), json!(uuid_c));
    message_a.headers.insert(
        "argsrepr".to_string(),
        json!(format!("('{uuid_a}', {{'id': '{uuid_b}'}})")),
    );
    let mut message_b = celery_message(
        "plane.tasks.issue_activity",
        json!([uuid_c, {"id": uuid_d}]),
    );
    message_b.headers.insert("id".to_string(), json!(uuid_e));
    message_b.headers.insert(
        "argsrepr".to_string(),
        json!(format!("('{uuid_c}', {{'id': '{uuid_d}'}})")),
    );
    a.normalize_celery(&mut message_a);
    b.normalize_celery(&mut message_b);
    checks.accepted(
        "celery ids, args and argsrepr",
        &compare_celery(&[message_a.clone()], &[message_b.clone()]),
    )?;

    let mut message_wrong_task = message_b.clone();
    message_wrong_task.task = Some("plane.tasks.something_else".to_string());
    checks.detected(
        "celery task name",
        "celery",
        &compare_celery(&[message_a.clone()], &[message_wrong_task]),
    )?;
    let mut message_wrong_args = message_b.clone();
    message_wrong_args.args = json!(["different"]);
    checks.detected(
        "celery args",
        "celery",
        &compare_celery(&[message_a.clone()], &[message_wrong_args]),
    )?;
    // A task enqueued either side of a second boundary differs only in
    // `epoch`; that is legitimate.
    let start_seconds = start.timestamp();
    let mut epoch_a = celery_message("plane.tasks.issue_activity", json!([]));
    epoch_a.kwargs = json!({"epoch": start_seconds});
    let mut epoch_b = celery_message("plane.tasks.issue_activity", json!([]));
    epoch_b.kwargs = json!({"epoch": start_seconds + 1});
    let mut epoch_a_normalised = epoch_a.clone();
    let mut epoch_b_normalised = epoch_b.clone();
    a.normalize_celery(&mut epoch_a_normalised);
    b.normalize_celery(&mut epoch_b_normalised);
    checks.accepted(
        "celery epoch one second apart",
        &compare_celery(&[epoch_a_normalised], &[epoch_b_normalised]),
    )?;
    let mut epoch_old = epoch_b.clone();
    epoch_old.kwargs = json!({"epoch": 1600000000});
    let mut epoch_old_normalised = epoch_old;
    a.normalize_celery(&mut epoch_old_normalised);
    checks.detected(
        "celery epoch from another period",
        "celery",
        &compare_celery(&[epoch_a.clone()], &[epoch_old_normalised]),
    )?;

    let mut message_wrong_header = message_b.clone();
    message_wrong_header
        .headers
        .insert("retries".to_string(), json!(1));
    checks.detected(
        "celery header",
        "celery",
        &compare_celery(&[message_a.clone()], &[message_wrong_header]),
    )?;
    checks.detected(
        "missing celery message",
        "celery",
        &compare_celery(&[message_a], &[]),
    )?;

    println!("selftest: comparator");
    // -- status and headers ------------------------------------------------
    checks.detected("status code", "status", &compare_status(200, 422))?;
    checks.accepted("same status", &compare_status(201, 201))?;

    let header_a = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Vary".to_string(), "Cookie".to_string()),
    ];
    let header_b = header_a.clone();
    checks.accepted("identical headers", &compare_headers(&header_a, &header_b))?;
    checks.detected(
        "header value",
        "headers",
        &compare_headers(
            &header_a,
            &[
                ("content-type".to_string(), "application/json".to_string()),
                ("vary".to_string(), "Origin".to_string()),
            ],
        ),
    )?;
    checks.detected(
        "missing header",
        "headers",
        &compare_headers(&header_a, &header_a[..1]),
    )?;

    // -- JSON bodies -------------------------------------------------------
    let identical = body(serde_json::from_str(r#"{"a":1,"b":[true,null,"x"]}"#).expect("json"));
    checks.accepted(
        "identical JSON",
        &compare_bodies(&identical, &identical.clone()),
    )?;
    checks.detected(
        "JSON value",
        "body",
        &compare_bodies(&body(json!({"a": 1})), &body(json!({"a": 2}))),
    )?;
    checks.detected(
        "JSON key order",
        "body",
        &compare_bodies(
            &body(serde_json::from_str(r#"{"a":1,"b":2}"#).expect("json")),
            &body(serde_json::from_str(r#"{"b":2,"a":1}"#).expect("json")),
        ),
    )?;
    checks.detected(
        "JSON number lexeme",
        "body",
        &compare_bodies(
            &body(serde_json::from_str(r#"{"n":100.0}"#).expect("json")),
            &body(serde_json::from_str(r#"{"n":100}"#).expect("json")),
        ),
    )?;
    checks.detected(
        "JSON missing key",
        "body",
        &compare_bodies(&body(json!({"a": 1})), &body(json!({}))),
    )?;
    checks.detected(
        "JSON array order",
        "body",
        &compare_bodies(&body(json!([1, 2])), &body(json!([2, 1]))),
    )?;

    // -- D15 byte rule -----------------------------------------------------
    let pass = check_drf_formatting(br#"{"a":1,"b":"x"}"#);
    let fail = check_drf_formatting(br#"{"a": 1, "b": "x"}"#);
    checks.ok("DRF-compact body passes", pass.is_ok())?;
    checks.ok("spaced body fails", fail.is_err())?;
    checks.detected(
        "formatting mismatch",
        "body",
        &compare_formatting(
            &FormattingStatus::Pass,
            &FormattingStatus::Fail("spaced".to_string()),
        ),
    )?;
    checks.accepted(
        "both sides non-compact (JsonResponse routes)",
        &compare_formatting(
            &FormattingStatus::Fail("spaced".to_string()),
            &FormattingStatus::Fail("spaced".to_string()),
        ),
    )?;

    let line_separator = "{\"a\":\"\u{2028}\"}";
    let escaped = "{\"a\":\"\\u2028\"}";
    checks.ok(
        "U+2028 must be escaped as six ASCII characters",
        check_drf_formatting(escaped.as_bytes()).is_ok()
            && check_drf_formatting(line_separator.as_bytes()).is_err(),
    )?;
    let paragraph_separator = "{\"a\":\"\u{2029}\"}";
    let escaped_p = "{\"a\":\"\\u2029\"}";
    checks.ok(
        "U+2029 must be escaped as six ASCII characters",
        check_drf_formatting(escaped_p.as_bytes()).is_ok()
            && check_drf_formatting(paragraph_separator.as_bytes()).is_err(),
    )?;
    checks.ok(
        "other non-ASCII is written raw",
        check_drf_formatting("{\"a\":\"héllo→\"}".as_bytes()).is_ok()
            && check_drf_formatting("{\"a\":\"h\\u00e9llo\"}".as_bytes()).is_err(),
    )?;
    checks.ok(
        "drf_compact keeps key order and number lexemes",
        drf_compact(&serde_json::from_str(r#"{"b":1.0,"a":2e3}"#).expect("json")).expect("compact")
            == r#"{"b":1.0,"a":2e+3}"#,
    )?;

    // -- text and binary bodies -------------------------------------------
    let text_a = NormalizedBody {
        value: BodyValue::Text("hello".to_string()),
        modified: false,
    };
    let text_b = NormalizedBody {
        value: BodyValue::Text("hello".to_string()),
        modified: false,
    };
    checks.accepted("identical text bodies", &compare_bodies(&text_a, &text_b))?;
    let text_c = NormalizedBody {
        value: BodyValue::Text("hello!".to_string()),
        modified: false,
    };
    checks.detected("text body", "body", &compare_bodies(&text_a, &text_c))?;
    let binary_a = NormalizedBody {
        value: BodyValue::Binary("00ff".to_string()),
        modified: false,
    };
    let binary_b = NormalizedBody {
        value: BodyValue::Binary("00fe".to_string()),
        modified: false,
    };
    checks.detected("binary body", "body", &compare_bodies(&binary_a, &binary_b))?;

    // -- ordering of placeholders across parts ----------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    a.start_step(start, start);
    let headers = a.normalize_headers(&[
        ("X-Second".to_string(), uuid_b.to_string()),
        ("Location".to_string(), format!("/{uuid_a}")),
    ]);
    checks.ok(
        "Location is assigned before other headers",
        headers[1].1 == "/<id:1>" && headers[0].1 == "<id:2>",
    )?;
    let body_after = a.normalize_body(format!(r#"["{uuid_c}"]"#).as_bytes(), true);
    checks.ok(
        "body continues the numbering after headers",
        body_after.pretty().contains("<id:3>"),
    )?;

    println!("selftest: {} checks passed", checks.count);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn selftest_passes() {
        super::run().expect("selftest must pass");
    }
}
